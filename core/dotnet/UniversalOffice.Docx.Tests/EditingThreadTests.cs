using UniversalOffice.Docx.Editing;
using UniversalOffice.Native;
using Xunit;
using Xunit.Abstractions;
using static UniversalOffice.Docx.Tests.EditingFixtures;

namespace UniversalOffice.Docx.Tests;

/// <summary>Threads: documents are independent, and one document is safe to share (one call at a time).</summary>
[Collection("memory")]
public class EditingThreadTests(ITestOutputHelper log)
{
    private static byte[] Flow(byte[] bytes, int i)
    {
        using var doc = DocxDocument.Open(bytes);
        DocxText.ReplaceParameter(doc, "Name", "thread " + i % 3);
        using var row = DocxTableRowTemplate.Take(doc, "#{Row}");
        row!.Append([new("Row", "r" + i % 3)]);
        return doc.Save();
    }

    [Fact]
    public void Eight_threads_with_their_own_documents_give_the_sequential_bytes()
    {
        var bytes = Small();
        var expected = Enumerable.Range(0, 3).Select(i => Flow(bytes, i)).ToArray();
        long before = BoundaryMemoryTests.Settle();
        var errors = new List<Exception>();
        var threads = Enumerable.Range(0, 8).Select(t => new Thread(() =>
        {
            try
            {
                for (int k = 0; k < 50; k++)
                {
                    int i = (t + k) % 3;
                    Assert.Equal(expected[i], Flow(bytes, i));
                }
            }
            catch (Exception e) { lock (errors) errors.Add(e); }
        })).ToList();
        threads.ForEach(t => t.Start());
        threads.ForEach(t => t.Join());
        Assert.Empty(errors);
        Assert.Equal(before, BoundaryMemoryTests.Settle());
        log.WriteLine("8 threads x 50 flows equal to the sequential bytes; native memory flat");
    }

    [Fact]
    public void Eight_threads_hammering_one_document_do_not_crash_and_leave_memory_flat()
    {
        var bytes = Small();
        long before = BoundaryMemoryTests.Settle();
        using (var doc = DocxDocument.Open(bytes))
        {
            var errors = new List<Exception>();
            var threads = Enumerable.Range(0, 8).Select(t => new Thread(() =>
            {
                try
                {
                    using var src = DocxDocument.Open(bytes);
                    for (int k = 0; k < 20; k++)
                    {
                        DocxText.ReplaceParameter(doc, "Name", "t" + t);
                        _ = DocxText.GetPlainText(doc);
                        _ = doc.Properties.Title;
                        doc.Properties.Title = "t" + t;
                        _ = doc.PartNames;
                        _ = doc.Save();
                        using var clone = doc.Clone();
                        DocxTemplate.ImportBody(doc, src);
                    }
                }
                catch (Exception e) { lock (errors) errors.Add(e); }
            })).ToList();
            threads.ForEach(t => t.Start());
            threads.ForEach(t => t.Join());
            Assert.Empty(errors);
        }
        Assert.Equal(before, BoundaryMemoryTests.Settle());
        log.WriteLine("8 threads x 20 rounds on one document: no crash, native memory flat");
    }

    [Fact]
    public void Disposing_while_other_threads_use_the_document_is_safe()
    {
        var bytes = Small();
        for (int round = 0; round < 20; round++)
        {
            var doc = DocxDocument.Open(bytes);
            var threads = Enumerable.Range(0, 4).Select(r => new Thread(() =>
            {
                try { for (int k = 0; k < 200; k++) _ = DocxText.GetPlainText(doc); }
                catch (ObjectDisposedException) { }
            })).ToList();
            threads.ForEach(t => t.Start());
            doc.Dispose();
            threads.ForEach(t => t.Join());
        }
        Assert.True(NativeCore.IsCountingMemory);
    }
}
