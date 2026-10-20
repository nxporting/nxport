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
} RtHost;

static RtHost host;

typedef struct {
    const uint64_t *addrs; // sorted
    const GuestFn *fns;
    uint64_t count;
} FnTable;

#define MAX_MODULES 32
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

// A direct-mapped cache in front of the lookup, per thread (the tables do not change once guest code runs).
#define CACHE_SIZE 4096
static __thread struct { uint64_t pc; GuestFn fn; } cache[CACHE_SIZE];

void rt_dispatch(Cpu *c) {
    uint64_t pc = c->pc;
    if (pc == RT_RETURN_TO_HOST) return;
    unsigned h = (unsigned)(pc >> 2) & (CACHE_SIZE - 1);
    if (cache[h].pc == pc && cache[h].fn) {
        __attribute__((musttail)) return cache[h].fn(c);
    }
    GuestFn f = lookup(pc);
    if (f) {
        cache[h].pc = pc;
        cache[h].fn = f;
        __attribute__((musttail)) return f(c);
    }
    // Not lifted code: the host runs it (an HLE call returns to x30, which the caller's RT_RETURNED check expects).
    host.call(c);
}

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
