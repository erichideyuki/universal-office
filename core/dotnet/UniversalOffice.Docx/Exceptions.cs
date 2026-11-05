namespace UniversalOffice;

/// <summary>An error of the native core that is not an argument, data, limit or cancellation error
/// (<see cref="Code"/>: 2 invalid operation, 11 no font, 12 internal, 13 output refused).</summary>
public class UniversalOfficeException : Exception
{
    public UniversalOfficeException(string message, int code, string stage) : base(message)
    {
        Code = code;
        Stage = stage;
    }

    /// <summary>The status code of the C ABI (record uo-bindings.md, "Error codes").</summary>
    public int Code { get; }

    /// <summary>"read", "layout", "pdf" or "" (before any stage).</summary>
    public string Stage { get; }
}

/// <summary>A limit for untrusted documents was hit (package, reader, layout or PDF): the document is
/// refused instead of exhausting the server.</summary>
public sealed class OfficeLimitExceededException : UniversalOfficeException
{
    public OfficeLimitExceededException(string message, string stage, long maximum) : base(message, 5, stage)
    {
        Maximum = maximum;
    }

    /// <summary>The configured maximum, when the limit says it (package and reader limits), else 0.</summary>
    public long Maximum { get; }
}
