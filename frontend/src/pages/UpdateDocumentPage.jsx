import { SavePlus } from "lucide-react";
import CustomTable from "../components/CustomTable";
import { useEffect, useState } from "react";

export default function UpdateDocumentPage({}) {
  const tableHeaders = ["Title", "Stable identifiers", "Published"];
  const [documents, setDocuments] = useState(null);

  useEffect(() => {
    fetch("/api/documents")
      .then((res) => res.json())
      .then(setDocuments);
  }, []);

  return (
    <div className="rounded-lg bg-white p-8 shadow-md text-primary flex flex-col items-center">
      <div className="flex flex-row gap-1.5 mb-1">
        <SavePlus className="size-4" />
        <h2 className="text-xs font-semibold">Update documents</h2>
      </div>
      <h1 className="text-lg text-black font-bold">
        Select a published document
      </h1>
      <p className="text-xs text-muted-foreground italic mt-2">
        Only documents that successfully passed the pipeline and reached the
        canonical graph are shown here.
      </p>
      <div className="mt-4">
        {documents && (
          <CustomTable headers={tableHeaders} documents={documents} />
        )}
      </div>
    </div>
  );
}
