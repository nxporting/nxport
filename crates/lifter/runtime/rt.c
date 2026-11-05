// The lifted code's side of the runtime, compiled into the guest library with the generated files: the dispatcher,
// and the calls generated code makes into the host (cpu.h), forwarded to the callbacks the host hands over at load
// (nx_rt_init). A DLL cannot import from the program that loads it, so the host passes them in.
#include "cpu.h"

typedef struct RtHost {
    // c->pc is in no lifted module: an HLE function or trampoline, or code the lifter did not see. Returns with c->pc
    // where execution continues (an HLE function's adapter sets it to x30).
    void (*call)(Cpu *c);
    void (*svc)(Cpu *c, uint32_t imm, uint64_t pc);
    // brk (kind 0) / undefined instruction (kind 1); does not return.
    void (*trap)(Cpu *c, uint32_t kind, uint32_t imm, uint64_t pc);
    uint64_t (*ticks)(void);
    // The HLE function guest code reaches by branching to pc, as a handle for call_hle; null if none.
    void *(*resolve_hle)(uint64_t pc);
    // Runs HLE function `h` (from resolve_hle) as call would.
    void (*call_hle)(void *h, Cpu *c);
    // The guest invalidated instruction cache: code it wrote through a mapping may now run.
    void (*code_changed)(uint64_t addr);
} RtHost;

static RtHost host;

typedef struct {
    const uint64_t *addrs; // sorted
    const GuestFn *fns;
    uint64_t count;
} FnTable;

#define MAX_MODULES 4096
static FnTable tables[MAX_MODULES];
static int table_count;

RT_EXPORT void nx_rt_init(const RtHost *h) { host = *h; }

// Registers a module's function table (<name>_fn_addrs / _fn_ptrs / _fn_count); before any guest code runs.
RT_EXPORT int nx_rt_add_module(const uint64_t *addrs, const GuestFn *fns, uint64_t count) {
    if (table_count == MAX_MODULES) return -1;
    tables[table_count++] = (FnTable){addrs, fns, count};
    return 0;
}

static GuestFn lookup(uint64_t pc) {
    for (int t = 0; t < table_count; t++) {
        const FnTable *ft = &tables[t];
        if (!ft->count || pc < ft->addrs[0] || pc > ft->addrs[ft->count - 1]) continue;
        uint64_t lo = 0, hi = ft->count;
        while (lo < hi) {
            uint64_t mid = (lo + hi) / 2;
            if (ft->addrs[mid] < pc) lo = mid + 1;
            else hi = mid;
        }
        if (lo < ft->count && ft->addrs[lo] == pc) return ft->fns[lo];
    }
    return 0;
}

// A direct-mapped cache in front of the lookup, per thread (the tables do not change once guest code runs). HLE
// targets are kept too (a binding never changes once guest code can reach it), as pc | 1 with the host's handle.

__thread RtCacheEntry rt_cache[RT_CACHE_SIZE];

#ifndef RT_CC
#define RT_CC
#endif

// Out of line, so that a hit saves no registers.
static RT_CC __attribute__((noinline)) void dispatch_miss(Cpu *c);

void rt_dispatch(Cpu *c) {
    uint64_t pc = c->pc;
    if (pc == RT_RETURN_TO_HOST) return;
    unsigned h = (unsigned)(pc >> 2) & (RT_CACHE_SIZE - 1);
    // A kept pc always has its function (pc 0 is never looked up).
    if (__builtin_expect(rt_cache[h].pc == pc, 1)) {
        __attribute__((musttail)) return rt_cache[h].fn(c);
    }
    if (rt_cache[h].pc == (pc | 1)) {
        host.call_hle((void *)rt_cache[h].fn, c);
        return;
    }
    __attribute__((musttail)) return dispatch_miss(c);
}

static RT_CC void dispatch_miss(Cpu *c) {
    uint64_t pc = c->pc;
    unsigned h = (unsigned)(pc >> 2) & (RT_CACHE_SIZE - 1);
    GuestFn f = lookup(pc);
    if (f) {
        rt_cache[h].pc = pc;
        rt_cache[h].fn = f;
        __attribute__((musttail)) return f(c);
    }
    void *hle = host.resolve_hle(pc);
    if (hle) {
        rt_cache[h].pc = pc | 1;
        rt_cache[h].fn = (GuestFn)hle;
        host.call_hle(hle, c);
        return;
    }
    // Not lifted code: the host runs it (an HLE call returns to x30, which the caller's RT_RETURNED check expects).
    host.call(c);
}

// The dispatcher, for code outside this library that continues in it (the engine's live patches).
RT_EXPORT GuestFn nx_rt_dispatcher(void) { return rt_dispatch; }

// The host's entry: runs guest code from c->pc until it returns to RT_RETURN_TO_HOST (x30 set by the host).
RT_EXPORT void nx_rt_enter(Cpu *c) {
    while (c->pc != RT_RETURN_TO_HOST) rt_dispatch(c);
}

void rt_svc(Cpu *c, uint32_t imm, uint64_t pc) { host.svc(c, imm, pc); }

void rt_brk(Cpu *c, uint32_t imm, uint64_t pc) {
    host.trap(c, 0, imm, pc);
    __builtin_unreachable();
}

void rt_udf(Cpu *c, uint64_t pc) {
    host.trap(c, 1, 0, pc);
    __builtin_unreachable();
}

uint64_t rt_ticks(void) { return host.ticks(); }
void rt_code_changed(uint64_t addr) { host.code_changed(addr); }
