using System.Text.Json;
using System.Xml;
using UniversalOffice.Core;

namespace UniversalOffice.Native;

/// <summary>The last Editing error of this thread as the .NET exception the 0.9 C# threw.</summary>
internal static unsafe class EditingErrors
{
    /// <summary>Throws for a non-zero status.</summary>
    internal static void Check(int status)
    {
        if (status != 0) throw Of(status);
    }

    internal static Exception Of(int status)
    {
        NativeMethods.ErrorInfo info;
        NativeMethods.LastError(&info);
        string message = "";
        if (info.MessageLen > 0)
        {
            var bytes = new byte[checked((int)info.MessageLen)];
            fixed (byte* p = bytes) NativeMethods.LastErrorMessage(p, (nuint)bytes.Length);
            message = System.Text.Encoding.UTF8.GetString(bytes);
        }
        NativeMethods.UoLimitInfo li;
        NativeMethods.LastErrorLimit(&li);
        string? part = null;
        if (li.PartLen != ulong.MaxValue)
        {
            var bytes = new byte[checked((int)li.PartLen)];
            fixed (byte* p = bytes) NativeMethods.LastErrorPart(p, (nuint)bytes.Length);
            part = System.Text.Encoding.UTF8.GetString(bytes);
        }
        // both slots keep no allocation once read (the memory gate counts them)
        NativeMethods.EditingErrorClear();
        if (message.Length == 0) message = $"status {status}";
        return status switch
        {
            1 => new ArgumentException(message),
            2 => new InvalidOperationException(message),
            3 => new InvalidDataException(message),
            4 => new XmlException(message),
            5 => new OfficeLimitExceededException(message, li.Limit is >= 0 and <= (int)OfficeLimit.Conversion ? (OfficeLimit)li.Limit : OfficeLimit.Conversion,
                (long)Math.Min(info.Max, long.MaxValue), part, ""),
            6 => new FormatException(message),
            7 => new OverflowException(message),
            8 => new JsonException(message),
            9 => new ArgumentOutOfRangeException(null, message),
            _ => new UniversalOfficeException(message, status, ""),
        };
    }
}
