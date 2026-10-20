using System;
using System.Collections.Generic;
using System.Linq;
using System.IO;
using System.Runtime.InteropServices;
using Ryujinx.Graphics.Shader;
using Ryujinx.Graphics.Shader.Translation;

namespace Sass2Spv;

public class MemoryGpuAccessor : IGpuAccessor
{
    private readonly ulong[] _code;
    private readonly bool _isCompute;
    // Whole NVN stage data (0x30 preamble + SPH + code + constant bank) and the constant bank's offset in it, or -1.
    private readonly byte[] _stage;
    private readonly int _cb1Offset;

    // Compute dispatch shape from the GLSLC control section (the QMD on hardware): workgroup size and memory sizes.
    public int LocalSizeX = 1, LocalSizeY = 1, LocalSizeZ = 1, SharedMemorySize = 0, LocalMemorySize = 0;

    public MemoryGpuAccessor(byte[] sassBytes, bool isCompute, byte[] stage = null, int cb1Offset = -1)
    {
        _isCompute = isCompute;
        int ulongCount = sassBytes.Length / 8;
        _code = new ulong[ulongCount];
        Buffer.BlockCopy(sassBytes, 0, _code, 0, ulongCount * 8);
        _stage = stage ?? Array.Empty<byte>();
        _cb1Offset = cb1Offset;
    }

    /// Offset of the constant bank in NVN stage data when the GLSLC control section is not available: the program ends
    /// with a branch-to-self followed by NOP padding to a 32-byte group; the bank starts at the next 0x100 boundary,
    /// after a zero gap. Returns -1 when that shape is not found.
    public static int FindConstBank(byte[] stage)
    {
        const ulong BraSelf = 0xe2400fffff07000fUL;
        const ulong Nop = 0x50b0000000070f00UL;
        const int CodeStart = 0x80;
        for (int q = CodeStart; q + 8 <= stage.Length; q += 8)
        {
            // BRA to itself (offset -1 instruction); bits 16..23 vary (predicate / flags).
            if ((BitConverter.ToUInt64(stage, q) & 0xffffffffff00ffffUL) != (BraSelf & 0xffffffffff00ffffUL)) continue;
            int g = CodeStart + ((q + 8 - CodeStart + 0x1f) / 0x20) * 0x20;
            while (g + 0x20 <= stage.Length)
            {
                bool allZero = true, padding = true;
                for (int w = 0; w < 4; w++)
                {
                    ulong v = BitConverter.ToUInt64(stage, g + w * 8);
                    if (v != 0) allZero = false;
                    if (w > 0 && v != Nop && v != 0) padding = false;
                }
                if (allZero || !padding) break;
                g += 0x20;
            }
            int bank = (g + 0xff) & ~0xff;
            bool gapZero = true;
            for (int i = g; i < bank && i < stage.Length; i++) if (stage[i] != 0) { gapZero = false; break; }
            if (gapZero && bank < stage.Length) return bank;
        }
        return -1;
    }

    public ReadOnlySpan<ulong> GetCode(ulong address, int minimumSize)
    {
        int index = (int)(address / 8);
        if (index >= _code.Length) return ReadOnlySpan<ulong>.Empty;
        return _code.AsSpan(index);
    }

    /// The decoder reads indirect-branch (BRX) jump tables from constant buffer 1 while translating.
    public uint ConstantBuffer1Read(int offset)
    {
        int at = _cb1Offset + offset;
        if (_cb1Offset < 0 || offset < 0 || at + 4 > _stage.Length) return 0;
        return BitConverter.ToUInt32(_stage, at);
    }

    // Binding numbers are consumed verbatim by spirv-cross (--msl-decoration-binding), so they must be
    // unique per Metal resource class: buffer(0..17) constant buffers, buffer(18..) storage buffers,
    // texture(0..) sampled textures, texture(ImageBase..) storage images.
    public const int StorageBufferBase = 18;
    public const int ImageBase = 96;
    private int _nextTexture;
    private int _nextImage;

    // Binding 0 belongs to Ryujinx's support buffer (render scales, viewport size), which shares the binding space
    // with the guest constant buffers; the game's driver cbuf 0 moves to the otherwise unused 17.
    public const int DriverCbufBinding = 17;
    public SetBindingPair CreateConstantBufferBinding(int index) =>
        _isCompute ? new SetBindingPair(0, _nextComputeBuffer++) : new SetBindingPair(0, index == 0 ? DriverCbufBinding : index);
    public SetBindingPair CreateImageBinding(int count, bool isBuffer)
    {
        int binding = ImageBase + _nextImage;
        _nextImage += count;
        return new SetBindingPair(1, binding);
    }
    public SetBindingPair CreateStorageBufferBinding(int index) =>
        _isCompute ? new SetBindingPair(2, _nextComputeBuffer++) : new SetBindingPair(2, StorageBufferBase + index);
    // Compute kernels have no vertex buffers, and may use up to 16 storage buffers: their constant and storage buffers
    // are numbered compactly from 1 (0 is the support buffer) so they fit Metal's 31 buffer slots.
    private int _nextComputeBuffer = 1;
    public SetBindingPair CreateTextureBinding(int count, bool isBuffer)
    {
        int binding = _nextTexture;
        _nextTexture += count;
        return new SetBindingPair(3, binding);
    }
    public int CreateExtraSet() => 4;

    // The Switch GPU's registers are untyped: vertex fetch and the ROP convert per the bound format and the
    // shader gives the bits meaning. Metal needs a declared type, so translate with raw 32-bit (uint) inputs and
    // outputs; crates/cli/src/msl_specialize.rs turns them into function-constant selected declarations that the engine
    // resolves from the real vertex / render target format at pipeline creation. No alpha test (NVN has none).
    public GpuGraphicsState QueryGraphicsState()
    {
        var attributeTypes = new Ryujinx.Common.Memory.Array32<AttributeType>();
        for (int i = 0; i < 32; i++) attributeTypes[i] = AttributeType.Uint;
        var outputTypes = new Ryujinx.Common.Memory.Array8<AttributeType>();
        for (int i = 0; i < 8; i++) outputTypes[i] = AttributeType.Uint;
        return new GpuGraphicsState(
            false, InputTopology.Points, false, TessPatchType.Triangles, TessSpacing.EqualSpacing,
            false, false, false, false, false, 1f, AlphaTestOp.Always, 0f,
            attributeTypes, true, outputTypes, false, false, false, false);
    }

    public int QueryComputeLocalSizeX() => LocalSizeX;
    public int QueryComputeLocalSizeY() => LocalSizeY;
    public int QueryComputeLocalSizeZ() => LocalSizeZ;
    public int QueryComputeLocalMemorySize() => LocalMemorySize;
    public int QueryComputeSharedMemorySize() => SharedMemorySize;

    public uint QueryConstantBufferUse() => uint.MaxValue;

    public bool QueryHasConstantBufferDrawParameters() => false;
    public bool QueryHasUnalignedStorageBuffer() => false;

    public void Log(string message)
    {
        if (Program.Opt("log") != null) Console.Error.WriteLine("[log] " + message);
    }

    public int QueryHostGatherBiasPrecision() => 0;
    public bool QueryHostReducedPrecision() => false;
    public bool QueryHostHasFrontFacingBug() => false;
    public bool QueryHostHasVectorIndexingBug() => false;
    public int QueryHostStorageBufferOffsetAlignment() => 16;
    public int QueryHostSubgroupSize() => 32; // Apple Silicon SIMD size

    public bool QueryHostSupportsBgraFormat() => true;
    public bool QueryHostSupportsFragmentShaderInterlock() => true;
    public bool QueryHostSupportsFragmentShaderOrderingIntel() => false;
    public bool QueryHostSupportsGeometryShader() => false;
    public bool QueryHostSupportsGeometryShaderPassthrough() => false;
    public bool QueryHostSupportsImageLoadFormatted() => true;
    public bool QueryHostSupportsLayerVertexTessellation() => true;
    public bool QueryHostSupportsNonConstantTextureOffset() => true;
    public bool QueryHostSupportsScaledVertexFormats() => true;
    public bool QueryHostSupportsSeparateSampler() => true;
    public bool QueryHostSupportsShaderBallot() => true;
    public bool QueryHostSupportsShaderBarrierDivergence() => true;
    public bool QueryHostSupportsShaderFloat64() => true;
    public bool QueryHostSupportsSnormBufferTextureFormat() => true;
    public bool QueryHostSupportsTextureGatherOffsets() => true;
    public bool QueryHostSupportsTextureShadowLod() => true;
    public bool QueryHostSupportsTransformFeedback() => false;
    public bool QueryHostSupportsViewportIndexVertexTessellation() => true;
    public bool QueryHostSupportsViewportMask() => false;
    public bool QueryHostSupportsDepthClipControl() => true;

    public int QuerySamplerArrayLengthFromPool() => 0;
    public SamplerType QuerySamplerType(int handle, int cbufSlot) => SamplerType.Texture2D;
    public int QueryTextureArrayLengthFromBuffer(int slot) => 0;
    public int QueryTextureArrayLengthFromPool() => 0;
    public bool QueryTextureCoordNormalized(int handle, int cbufSlot) => true;
    public TextureFormat QueryTextureFormat(int handle, int cbufSlot) => TextureFormat.R8G8B8A8Unorm;

    public bool QueryTransformFeedbackEnabled() => false;
    public ReadOnlySpan<byte> QueryTransformFeedbackVaryingLocations(int bufferIndex) => ReadOnlySpan<byte>.Empty;
    public int QueryTransformFeedbackStride(int bufferIndex) => 0;

    public void RegisterTexture(int handle, int cbufSlot) { }
}

public class Program
{
    // Resource manifest consumed by the engine: which guest constant buffer / texture handle feeds each
    // Metal binding of the translated stage.
    private static void WriteManifest(string path, ShaderProgramInfo info)
    {
        var sb = new System.Text.StringBuilder();
        sb.Append("{\"stage\":\"").Append(info.Stage).Append("\"");
        sb.Append(",\"usesInstanceId\":").Append(info.UsesInstanceId ? "true" : "false");
        sb.Append(",\"usesDrawParameters\":").Append(info.UsesDrawParameters ? "true" : "false");
        sb.Append(",\"cbuffers\":[");
        sb.Append(string.Join(",", info.CBuffers.Select(b => $"{{\"binding\":{b.Binding},\"slot\":{b.Slot}}}")));
        sb.Append("],\"sbuffers\":[");
        sb.Append(string.Join(",", info.SBuffers.Select(b => $"{{\"binding\":{b.Binding},\"slot\":{b.Slot},\"cbSlot\":{b.SbCbSlot},\"cbOffset\":{b.SbCbOffset}}}")));
        sb.Append("],\"textures\":[");
        sb.Append(string.Join(",", info.Textures.Select(t => $"{{\"binding\":{t.Binding},\"cbufSlot\":{t.CbufSlot},\"handleIndex\":{t.HandleIndex},\"type\":\"{t.Type}\",\"arrayLength\":{t.ArrayLength}}}")));
        sb.Append("],\"images\":[");
        sb.Append(string.Join(",", info.Images.Select(t => $"{{\"binding\":{t.Binding},\"cbufSlot\":{t.CbufSlot},\"handleIndex\":{t.HandleIndex},\"type\":\"{t.Type}\"}}")));
        sb.Append("]}");
        File.WriteAllText(path, sb.ToString());
    }

    /// Translates every stage listed in `listFile` (one path per line) in this process, writing <outDir>/<name>.spv and
    /// <name>.spv.json, and prints one tab-separated RESULT line per stage. Starting the .NET runtime and the decompiler
    /// dominated the cost of translating thousands of stages one process at a time.
    private static int RunBatch(string listFile, string outDir)
    {
        Directory.CreateDirectory(outDir);
        foreach (string line in File.ReadLines(listFile))
        {
            string[] fields = line.Trim().Split('\t');
            string path = fields[0];
            if (path.Length == 0) continue;
            string name = Path.GetFileNameWithoutExtension(path);
            try
            {
                byte[] stage = File.ReadAllBytes(path);
                byte[] sass = stage;
                // Graphics stages: 0x12345678 preamble (0x30 bytes) then the SPH. Compute stages: 0x12345679 preamble,
                // no SPH, code at 0x100; or, from older GLSLC versions (Kirby), no preamble at all: the builder then marks
                // them compute by passing the workgroup fields (2..6) from the control section.
                uint magic = sass.Length >= 4 ? BitConverter.ToUInt32(sass, 0) : 0;
                bool compute = (sass.Length > 0x100 && magic == 0x12345679) || (magic != 0x12345678 && fields.Length > 2);
                if (compute && magic == 0x12345679) sass = sass[0x100..];
                else if (sass.Length > 0x30 && magic == 0x12345678) sass = sass[0x30..];
                // Constant bank offset from the stage's control section when the builder knows it, else the heuristic.
                int cb1 = fields.Length > 1 && int.TryParse(fields[1], out int c) && c > 0 ? c : MemoryGpuAccessor.FindConstBank(stage);
                var accessor = new MemoryGpuAccessor(sass, compute, stage, cb1);
                if (compute)
                {
                    // Fields 2..6: local size x, y, z, shared memory bytes, local memory bytes.
                    int F(int i, int d) => fields.Length > i && int.TryParse(fields[i], out int v) && v > 0 ? v : d;
                    accessor.LocalSizeX = F(2, 1);
                    accessor.LocalSizeY = F(3, 1);
                    accessor.LocalSizeZ = F(4, 1);
                    accessor.SharedMemorySize = F(5, 0);
                    accessor.LocalMemorySize = F(6, 0);
                }
                var options = new TranslationOptions(TargetLanguage.Spirv, TargetApi.Vulkan, compute ? TranslationFlags.Compute : TranslationFlags.None);
                var context = Translator.CreateContext(0, accessor, options);
                ShaderProgram program = context.Translate(asCompute: false);
                if (program.BinaryCode == null || program.BinaryCode.Length == 0)
                {
                    Console.WriteLine($"RESULT\t{name}\tFAIL\tempty binary");
                    continue;
                }
                string spv = Path.Combine(outDir, name + ".spv");
                File.WriteAllBytes(spv, program.BinaryCode);
                WriteManifest(spv + ".json", program.Info);
                Console.WriteLine($"RESULT\t{name}\tOK\t{context.Stage}\t{context.Size}\t{(compute ? 0x100 : 0x30)}");
            }
            catch (Exception ex)
            {
                if (Program.Opt("trace") != null) Console.Error.WriteLine(ex);
                string first = (ex.ToString().Split('\n')[0]).Replace('\t', ' ').Trim();
                Console.WriteLine($"RESULT\t{name}\tFAIL\t{first}");
            }
        }
        return 0;
    }

    /// Options given as --name or --name=value (a bare flag reads as "1").
    static readonly Dictionary<string, string> Opts = new();
    public static string? Opt(string name) => Opts.TryGetValue(name, out var v) ? v : null;

    public static int Main(string[] rawArgs)
    {
        var positional = new List<string>();
        foreach (var a in rawArgs)
        {
            if (a.StartsWith("--") && a != "--batch" && a != "--compute")
            {
                int eq = a.IndexOf('=');
                if (eq < 0) Opts[a[2..]] = "1"; else Opts[a[2..eq]] = a[(eq + 1)..];
            }
            else positional.Add(a);
        }
        string[] args = positional.ToArray();
        if (args.Length == 3 && args[0] == "--batch")
        {
            return RunBatch(args[1], args[2]);
        }

        if (args.Length < 2)
        {
            Console.WriteLine("Usage: sass2spv <input.sass> <output.spv> [--compute] [--cb1=OFFSET] [--local=X,Y,Z,SHARED] [--glsl [--debug]]\n       sass2spv --batch <list.txt> <outdir>\n       --log: translator warnings; --trace: full exceptions");
            return 1;
        }

        string inputPath = args[0];
        string outputPath = args[1];
        bool isCompute = args.Length > 2 && args[2] == "--compute";

        Console.WriteLine($"[+] Loading SASS: {inputPath}");
        byte[] sassBytes = File.ReadAllBytes(inputPath);
        // NVN stage data is {0x30-byte preamble starting with 0x12345678, 0x50-byte SPH, code};
        // Ryujinx expects the buffer to start at the SPH.
        if (sassBytes.Length > 0x100 && BitConverter.ToUInt32(sassBytes, 0) == 0x12345679)
        {
            sassBytes = sassBytes[0x100..]; // compute stage: no SPH, code at 0x100
            isCompute = true;
        }
        else if (sassBytes.Length > 0x30 && BitConverter.ToUInt32(sassBytes, 0) == 0x12345678)
        {
            sassBytes = sassBytes[0x30..];
        }
        Console.WriteLine($"[+] SASS byte length: {sassBytes.Length}");

        byte[] stageData = File.ReadAllBytes(inputPath);
        int cb1Env = int.TryParse(Opt("cb1") ?? "", out int ce) ? ce : -1;
        int cb1Off = cb1Env > 0 ? cb1Env : MemoryGpuAccessor.FindConstBank(stageData);
        Console.WriteLine($"[+] Constant bank (cbuf 1) at stage offset 0x{cb1Off:x}");
        var accessor = new MemoryGpuAccessor(sassBytes, isCompute, stageData, cb1Off);
        if (Opt("local") is string ls)
        {
            int[] v = ls.Split(',').Select(int.Parse).ToArray();
            (accessor.LocalSizeX, accessor.LocalSizeY, accessor.LocalSizeZ, accessor.SharedMemorySize, accessor.LocalMemorySize) = (v[0], v[1], v[2], v[3], v[3]);
        }
        var flags = isCompute ? TranslationFlags.Compute : TranslationFlags.None;
        // --glsl: emit Ryujinx's GLSL instead (debugging the decompiler's control flow without SPIR-V).
        bool glsl = Opt("glsl") != null;
        if (glsl && Opt("debug") != null) flags |= TranslationFlags.DebugMode;
        var options = new TranslationOptions(glsl ? TargetLanguage.Glsl : TargetLanguage.Spirv, glsl ? TargetApi.OpenGL : TargetApi.Vulkan, flags);

        try
        {
            Console.WriteLine("[+] Creating TranslatorContext...");
            var context = Translator.CreateContext(0, accessor, options);
            Console.WriteLine($"[+] Detected Stage: {context.Stage}, Size: {context.Size} bytes");

            Console.WriteLine("[+] Translating to SPIR-V...");
            ShaderProgram program = context.Translate(asCompute: false);
            if (glsl)
            {
                File.WriteAllText(outputPath, program.Code);
                Console.WriteLine($"[+] SUCCESS: Emitted GLSL to {outputPath}");
                return 0;
            }

            if (program.BinaryCode != null && program.BinaryCode.Length > 0)
            {
                File.WriteAllBytes(outputPath, program.BinaryCode);
                WriteManifest(outputPath + ".json", program.Info);
                Console.WriteLine($"[+] SUCCESS: Emitted SPIR-V binary to {outputPath} ({program.BinaryCode.Length} bytes)");
                return 0;
            }
            else
            {
                Console.WriteLine("[-] Error: Translator emitted empty binary code.");
                return 2;
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[-] Translation Exception:\n{ex}");
            return 3;
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "sass2spv_csharp_free")]
    public static unsafe void CFree(byte* ptr)
    {
        if (ptr != null)
        {
            NativeMemory.Free(ptr);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "sass2spv_csharp_translate")]
    public static unsafe int CTranslate(
        byte* stagePtr,
        int stageLen,
        int stageHint, // 0 = auto/sph, 1 = fragment, 2 = vertex, 3 = compute
        int cb1Hint, // constant bank 1 offset from control section (+0x700), or <= 0 for heuristic
        int* computeFields, // compute stages: workgroup size x, y, z and shared memory bytes (control section), else null
        byte** outSpv,
        int* outSpvLen,
        byte** outManifestJson,
        int* outManifestLen,
        int* outSize,
        int* outStage)
    {
        try
        {
            if (stagePtr == null || stageLen <= 0) return -1;
            byte[] stage = new byte[stageLen];
            Marshal.Copy((IntPtr)stagePtr, stage, 0, stageLen);
            byte[] sass = stage;
            uint magic = sass.Length >= 4 ? BitConverter.ToUInt32(sass, 0) : 0;
            bool compute = (sass.Length > 0x100 && magic == 0x12345679) || (magic != 0x12345678 && stageHint == 3);
            if (compute && magic == 0x12345679) sass = sass[0x100..];
            else if (sass.Length > 0x30 && magic == 0x12345678) sass = sass[0x30..];
            int cb1Off = cb1Hint > 0 ? cb1Hint : MemoryGpuAccessor.FindConstBank(stage);
            var accessor = new MemoryGpuAccessor(sass, compute, stage, cb1Off);
            if (computeFields != null)
            {
                accessor.LocalSizeX = Math.Max(computeFields[0], 1);
                accessor.LocalSizeY = Math.Max(computeFields[1], 1);
                accessor.LocalSizeZ = Math.Max(computeFields[2], 1);
                accessor.SharedMemorySize = computeFields[3];
                accessor.LocalMemorySize = computeFields[3];
            }
            var options = new TranslationOptions(TargetLanguage.Spirv, TargetApi.Vulkan, compute ? TranslationFlags.Compute : TranslationFlags.None);
            var context = Translator.CreateContext(0, accessor, options);
            ShaderProgram program = context.Translate(asCompute: false);
            if (program.BinaryCode == null || program.BinaryCode.Length == 0)
            {
                return -2;
            }

            // Allocate and copy SPIR-V
            byte* spvNative = (byte*)NativeMemory.Alloc((nuint)program.BinaryCode.Length);
            Marshal.Copy(program.BinaryCode, 0, (IntPtr)spvNative, program.BinaryCode.Length);
            *outSpv = spvNative;
            *outSpvLen = program.BinaryCode.Length;

            // Generate Manifest JSON string
            var sb = new System.Text.StringBuilder();
            sb.Append("{\"stage\":\"").Append(program.Info.Stage).Append("\"");
            sb.Append(",\"usesInstanceId\":").Append(program.Info.UsesInstanceId ? "true" : "false");
            sb.Append(",\"usesDrawParameters\":").Append(program.Info.UsesDrawParameters ? "true" : "false");
            sb.Append(",\"codeEnd\":").Append(context.Size);
            sb.Append(",\"cbuffers\":[");
            sb.Append(string.Join(",", program.Info.CBuffers.Select(b => $"{{\"binding\":{b.Binding},\"slot\":{b.Slot}}}")));
            sb.Append("],\"sbuffers\":[");
            sb.Append(string.Join(",", program.Info.SBuffers.Select(b => $"{{\"binding\":{b.Binding},\"slot\":{b.Slot},\"cbSlot\":{b.SbCbSlot},\"cbOffset\":{b.SbCbOffset}}}")));
            sb.Append("],\"textures\":[");
            sb.Append(string.Join(",", program.Info.Textures.Select(t => $"{{\"binding\":{t.Binding},\"cbufSlot\":{t.CbufSlot},\"handleIndex\":{t.HandleIndex},\"type\":\"{t.Type}\",\"arrayLength\":{t.ArrayLength}}}")));
            sb.Append("],\"images\":[");
            sb.Append(string.Join(",", program.Info.Images.Select(t => $"{{\"binding\":{t.Binding},\"cbufSlot\":{t.CbufSlot},\"handleIndex\":{t.HandleIndex},\"type\":\"{t.Type}\"}}")));
            sb.Append("]}");

            byte[] jsonBytes = System.Text.Encoding.UTF8.GetBytes(sb.ToString());
            byte* jsonNative = (byte*)NativeMemory.Alloc((nuint)jsonBytes.Length);
            Marshal.Copy(jsonBytes, 0, (IntPtr)jsonNative, jsonBytes.Length);
            *outManifestJson = jsonNative;
            *outManifestLen = jsonBytes.Length;

            *outSize = context.Size;
            *outStage = (int)context.Stage;
            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"[sass2spv-native] Exception: {ex.Message}");
            return -3;
        }
    }
}
