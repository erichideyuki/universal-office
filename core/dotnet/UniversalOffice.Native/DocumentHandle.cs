using System.Runtime.InteropServices;

namespace UniversalOffice.Native;

/// <summary>Owns a native document of the Rust core: released exactly once, by
/// <c>Dispose()</c> or by the finalizer, through <c>uo_document_free</c>.</summary>
internal sealed class DocumentHandle : SafeHandle
{
    public DocumentHandle() : base(nint.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == nint.Zero;

    protected override bool ReleaseHandle()
    {
        NativeMethods.DocumentFree(handle);
        return true;
    }
}
