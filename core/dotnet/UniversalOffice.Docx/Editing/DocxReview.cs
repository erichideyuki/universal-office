namespace UniversalOffice.Docx.Editing;

/// <summary>Review marks: tracked changes, comments, protection, attached template and macros.</summary>
public static class DocxReview
{
    /// <summary>Accepts every tracked change.</summary>
    public static void AcceptAllRevisions(DocxDocument doc) => DocxOps.Run(doc, 1);

    /// <summary>Removes comments (marks, references and parts).</summary>
    public static void RemoveComments(DocxDocument doc) => DocxOps.Run(doc, 2);

    /// <summary>Turns tracking off.</summary>
    public static void DisableTrackChanges(DocxDocument doc) => DocxOps.Run(doc, 3);

    /// <summary>Removes document protection.</summary>
    public static void RemoveProtection(DocxDocument doc) => DocxOps.Run(doc, 4);

    /// <summary>Removes the attached template.</summary>
    public static void RemoveAttachedTemplate(DocxDocument doc) => DocxOps.Run(doc, 5);

    /// <summary>Removes macros/VBA and saves as a plain .docx.</summary>
    public static void RemoveMacros(DocxDocument doc) => DocxOps.Run(doc, 6);
}
