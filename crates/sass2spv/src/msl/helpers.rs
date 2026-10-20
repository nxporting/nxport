// Helper functions the generated code calls, with the text SPIRV-Cross 1.4.363 emits for them (the cache builder's
// later passes rewrite some of these bodies by their exact text). Declared in this order, as SPIRV-Cross does.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Helper {
    Mod,
    FindILsb,
    FindSMsb,
    FindUMsb,
    FMul,
    FAdd,
    FSub,
    UnsafeArray,
    GatherReturn,
    GatherCompareReturn,
    GatherConstOffsets,
    GatherCompareConstOffsets,
    SubgroupBallot,
    SubgroupAllEqual,
    SubgroupShuffle,
    SubgroupShuffleXor,
    SubgroupShuffleUp,
    SubgroupShuffleDown,
    DepthCast,
}

pub const UNSAFE_ARRAY: &str = "template<typename T, size_t Num>
struct spvUnsafeArray
{
    T elements[Num ? Num : 1];

    thread T& operator [] (size_t pos) thread
    {
        return elements[pos];
    }
    constexpr const thread T& operator [] (size_t pos) const thread
    {
        return elements[pos];
    }

    device T& operator [] (size_t pos) device
    {
        return elements[pos];
    }
    constexpr const device T& operator [] (size_t pos) const device
    {
        return elements[pos];
    }

    constexpr const constant T& operator [] (size_t pos) const constant
    {
        return elements[pos];
    }

    threadgroup T& operator [] (size_t pos) threadgroup
    {
        return elements[pos];
    }
    constexpr const threadgroup T& operator [] (size_t pos) const threadgroup
    {
        return elements[pos];
    }
};

";

const MOD: &str = "// Implementation of the GLSL mod() function, which is slightly different than Metal fmod()
template<typename Tx, typename Ty>
inline Tx mod(Tx x, Ty y)
{
    return x - y * floor(x / y);
}

";

const FIND_ILSB: &str = "// Implementation of the GLSL findLSB() function
template<typename T>
inline T spvFindLSB(T x)
{
    return select(ctz(x), T(-1), x == T(0));
}

";

const FIND_SMSB: &str = "// Implementation of the signed GLSL findMSB() function
template<typename T>
inline T spvFindSMSB(T x)
{
    T v = select(x, T(-1) - x, x < T(0));
    return select(clz(T(0)) - (clz(v) + T(1)), T(-1), v == T(0));
}

";

const FIND_UMSB: &str = "// Implementation of the unsigned GLSL findMSB() function
template<typename T>
inline T spvFindUMSB(T x)
{
    return select(clz(T(0)) - (clz(x) + T(1)), T(-1), x == T(0));
}

";

const FMUL: &str = "template<typename T>
[[clang::optnone]] T spvFMul(T l, T r)
{
    return fma(l, r, T(0));
}

template<typename T, int Cols, int Rows>
[[clang::optnone]] vec<T, Cols> spvFMulVectorMatrix(vec<T, Rows> v, matrix<T, Cols, Rows> m)
{
    vec<T, Cols> res = vec<T, Cols>(0);
    for (uint i = Rows; i > 0; --i)
    {
        vec<T, Cols> tmp(0);
        for (uint j = 0; j < Cols; ++j)
        {
            tmp[j] = m[j][i - 1];
        }
        res = fma(tmp, vec<T, Cols>(v[i - 1]), res);
    }
    return res;
}

template<typename T, int Cols, int Rows>
[[clang::optnone]] vec<T, Rows> spvFMulMatrixVector(matrix<T, Cols, Rows> m, vec<T, Cols> v)
{
    vec<T, Rows> res = vec<T, Rows>(0);
    for (uint i = Cols; i > 0; --i)
    {
        res = fma(m[i - 1], vec<T, Rows>(v[i - 1]), res);
    }
    return res;
}

template<typename T, int LCols, int LRows, int RCols, int RRows>
[[clang::optnone]] matrix<T, RCols, LRows> spvFMulMatrixMatrix(matrix<T, LCols, LRows> l, matrix<T, RCols, RRows> r)
{
    static_assert(LCols == RRows, \"column-row configuration mismatch\");
    matrix<T, RCols, LRows> res;
    for (uint i = 0; i < RCols; i++)
    {
        vec<T, LRows> tmp(0);
        for (uint j = 0; j < LCols; j++)
        {
            tmp = fma(vec<T, LRows>(r[i][j]), l[j], tmp);
        }
        res[i] = tmp;
    }
    return res;
}

";

const FADD: &str = "template<typename T>
[[clang::optnone]] T spvFAdd(T l, T r)
{
    return fma(T(1), l, r);
}

";

const FSUB: &str = "template<typename T>
[[clang::optnone]] T spvFSub(T l, T r)
{
    return fma(T(-1), r, l);
}

";

const GATHER_RETURN: &str = "template<typename Tex, typename... Tp>
using spvGatherReturn = decltype(declval<Tex>().gather(declval<sampler>(), declval<Tp>()...));

";

const GATHER_COMPARE_RETURN: &str = "template<typename Tex, typename... Tp>
using spvGatherCompareReturn = decltype(declval<Tex>().gather_compare(declval<sampler>(), declval<Tp>()...));

";

const TEXTURE_ADDR_SPACES: [&str; 3] = ["device", "constant", "thread"];

fn gather_const_offsets() -> String {
    let mut s = String::new();
    for a in TEXTURE_ADDR_SPACES {
        s += &format!(
            "// Wrapper function that processes a {a} texture gather with a constant offset array.
template<typename Tex, typename Toff, typename... Tp>
inline spvGatherReturn<Tex, Tp...> spvGatherConstOffsets(const {a} Tex& t, sampler s, Toff coffsets, component c, Tp... params) METAL_CONST_ARG(c)
{{
    spvGatherReturn<Tex, Tp...> rslts[4];
    for (uint i = 0; i < 4; i++)
    {{
        switch (c)
        {{
            case component::x:
                rslts[i] = t.gather(s, params..., coffsets[i], component::x);
                break;
            case component::y:
                rslts[i] = t.gather(s, params..., coffsets[i], component::y);
                break;
            case component::z:
                rslts[i] = t.gather(s, params..., coffsets[i], component::z);
                break;
            case component::w:
                rslts[i] = t.gather(s, params..., coffsets[i], component::w);
                break;
        }}
    }}
    return spvGatherReturn<Tex, Tp...>(rslts[0].w, rslts[1].w, rslts[2].w, rslts[3].w);
}}

"
        );
    }
    s
}

fn gather_compare_const_offsets() -> String {
    let mut s = String::new();
    for a in TEXTURE_ADDR_SPACES {
        s += &format!(
            "// Wrapper function that processes a {a} texture gather with a constant offset array.
template<typename Tex, typename Toff, typename... Tp>
inline spvGatherCompareReturn<Tex, Tp...> spvGatherCompareConstOffsets(const {a} Tex& t, sampler s, Toff coffsets, Tp... params)
{{
    spvGatherCompareReturn<Tex, Tp...> rslts[4];
    for (uint i = 0; i < 4; i++)
    {{
            rslts[i] = t.gather_compare(s, params..., coffsets[i]);
    }}
    return spvGatherCompareReturn<Tex, Tp...>(rslts[0].w, rslts[1].w, rslts[2].w, rslts[3].w);
}}

"
        );
    }
    s
}

const SUBGROUP_BALLOT: &str = "inline uint4 spvSubgroupBallot(bool value)
{
    simd_vote vote = simd_ballot(value);
    // simd_ballot() returns a 64-bit integer-like object, but
    // SPIR-V callers expect a uint4. We must convert.
    // FIXME: This won't include higher bits if Apple ever supports
    // 128 lanes in an SIMD-group.
    return uint4(as_type<uint2>((simd_vote::vote_t)vote), 0, 0);
}

";

const SUBGROUP_ALL_EQUAL: &str = "template<typename T>
inline bool spvSubgroupAllEqual(T value)
{
    return simd_all(all(value == simd_broadcast_first(value)));
}

template<>
inline bool spvSubgroupAllEqual(bool value)
{
    return simd_all(value) || !simd_any(value);
}

template<uint N>
inline bool spvSubgroupAllEqual(vec<bool, N> value)
{
    return simd_all(all(value == (vec<bool, N>)simd_broadcast_first((vec<ushort, N>)value)));
}

";

const SUBGROUP_SHUFFLE: &str = "template<typename T>
inline T spvSubgroupShuffle(T value, ushort lane)
{
    return simd_shuffle(value, lane);
}

template<>
inline bool spvSubgroupShuffle(bool value, ushort lane)
{
    return !!simd_shuffle((ushort)value, lane);
}

template<uint N>
inline vec<bool, N> spvSubgroupShuffle(vec<bool, N> value, ushort lane)
{
    return (vec<bool, N>)simd_shuffle((vec<ushort, N>)value, lane);
}

template<>
inline ulong spvSubgroupShuffle(ulong value, ushort lane)
{
    return as_type<ulong>(spvSubgroupShuffle(as_type<uint2>(value), lane));
}

template<>
inline ulong2 spvSubgroupShuffle(ulong2 value, ushort lane)
{
    return ulong2(spvSubgroupShuffle(value.x, lane), spvSubgroupShuffle(value.y, lane));
}

inline ulong3 spvSubgroupShuffle(ulong3 value, ushort lane)
{
    return ulong3(spvSubgroupShuffle(value.xy, lane), spvSubgroupShuffle(value.z, lane));
}

inline ulong4 spvSubgroupShuffle(ulong4 value, ushort lane)
{
    return ulong4(spvSubgroupShuffle(value.xy, lane), spvSubgroupShuffle(value.zw, lane));
}

template<uint N>
inline vec<long, N> spvSubgroupShuffle(vec<long, N> value, ushort lane)
{
    return vec<long, N>(spvSubgroupShuffle(vec<ulong, N>(value), lane));
}

";

fn subgroup_shuffle_variant(name: &str, metal: &str, arg: &str) -> String {
    format!(
        "template<typename T>
inline T {name}(T value, ushort {arg})
{{
    return {metal}(value, {arg});
}}

template<>
inline bool {name}(bool value, ushort {arg})
{{
    return !!{metal}((ushort)value, {arg});
}}

template<uint N>
inline vec<bool, N> {name}(vec<bool, N> value, ushort {arg})
{{
    return (vec<bool, N>){metal}((vec<ushort, N>)value, {arg});
}}

"
    )
}

const DEPTH_CAST: &str = "template <typename T>
static inline depth2d<T> spvDepthCast(texture2d<T> t)
{
    return reinterpret_cast<thread const depth2d<T> &>(t);
}

template <typename T>
static inline depth2d_array<T> spvDepthCast(texture2d_array<T> t)
{
    return reinterpret_cast<thread const depth2d_array<T> &>(t);
}

template <typename T>
static inline depthcube<T> spvDepthCast(texturecube<T> t)
{
    return reinterpret_cast<thread const depthcube<T> &>(t);
}

template <typename T>
static inline depthcube_array<T> spvDepthCast(texturecube_array<T> t)
{
    return reinterpret_cast<thread const depthcube_array<T> &>(t);
}

";

/// The text of one helper (UnsafeArray is a template emitted before the others: see UNSAFE_ARRAY).
pub fn text(h: Helper) -> String {
    match h {
        Helper::Mod => MOD.into(),
        Helper::FindILsb => FIND_ILSB.into(),
        Helper::FindSMsb => FIND_SMSB.into(),
        Helper::FindUMsb => FIND_UMSB.into(),
        Helper::FMul => FMUL.into(),
        Helper::FAdd => FADD.into(),
        Helper::FSub => FSUB.into(),
        Helper::UnsafeArray => String::new(),
        Helper::GatherReturn => GATHER_RETURN.into(),
        Helper::GatherCompareReturn => GATHER_COMPARE_RETURN.into(),
        Helper::GatherConstOffsets => gather_const_offsets(),
        Helper::GatherCompareConstOffsets => gather_compare_const_offsets(),
        Helper::SubgroupBallot => SUBGROUP_BALLOT.into(),
        Helper::SubgroupAllEqual => SUBGROUP_ALL_EQUAL.into(),
        Helper::SubgroupShuffle => SUBGROUP_SHUFFLE.into(),
        Helper::SubgroupShuffleXor => subgroup_shuffle_variant("spvSubgroupShuffleXor", "simd_shuffle_xor", "mask"),
        Helper::SubgroupShuffleUp => subgroup_shuffle_variant("spvSubgroupShuffleUp", "simd_shuffle_up", "delta"),
        Helper::SubgroupShuffleDown => subgroup_shuffle_variant("spvSubgroupShuffleDown", "simd_shuffle_down", "delta"),
        Helper::DepthCast => DEPTH_CAST.into(),
    }
}
