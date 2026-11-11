using System.Buffers.Binary;
using System.Text;

namespace UniversalOffice.Tests;

/// <summary>The one helper of the frozen OpcPackageLimitsTests (a file that needs the internal OpcPackage, so it
/// cannot be linked) that DocxUntrustedInputTests uses: a verbatim copy of LieAboutSize.</summary>
public class OpcPackageLimitsTests
{
    /// <summary>Changes the DECLARED uncompressed size (local header and central directory) of the entries with the prefix.</summary>
    internal static byte[] LieAboutSize(byte[] zip, string namePrefix, uint declared)
    {
        var bytes = (byte[])zip.Clone();
        for (int i = 0; i + 4 <= bytes.Length; i++)
        {
            uint sig = BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(i));
            if (sig == 0x04034b50 && i + 30 <= bytes.Length)
            {
                int nameLen = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(i + 26));
                if (Encoding.UTF8.GetString(bytes, i + 30, nameLen).StartsWith(namePrefix, StringComparison.Ordinal))
                    BinaryPrimitives.WriteUInt32LittleEndian(bytes.AsSpan(i + 22), declared);
            }
            else if (sig == 0x02014b50 && i + 46 <= bytes.Length)
            {
                int nameLen = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(i + 28));
                if (Encoding.UTF8.GetString(bytes, i + 46, nameLen).StartsWith(namePrefix, StringComparison.Ordinal))
                    BinaryPrimitives.WriteUInt32LittleEndian(bytes.AsSpan(i + 24), declared);
            }
        }
        return bytes;
    }
}
