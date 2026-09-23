import { ChevronLeft, File, FileText, Loader2, Upload, X } from "lucide-react";
import { useRef, useState } from "react";
import { NavLink, useNavigate } from "react-router-dom";

export default function UploadDocumentPage() {
  const [file, setFile] = useState(null);
  const inputRef = useRef(null);
  const [error, setError] = useState("");
  const [uploading, setUploading] = useState(false);
  const navigate = useNavigate();

  const allowedFileTypes = [
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  ];

  const handleInputChange = (e) => {
    const file = e.target.files?.[0];

    if (!file) return;

    if (allowedFileTypes.includes(file.type)) {
      setFile(file);
      setError("");
    } else {
      setError("Document must be a PDF or Word document");
    }
  };

  async function handleFileUpload(file) {
    setUploading(true);
    setError("");
    fetch("/api/pdfs", {
      method: "POST",
      headers: { "Content-Type": "application/pdf" },
      body: file,
    })
      .then((response) =>
        response.text().then((text) => {
          let data;
          try {
            data = text ? JSON.parse(text) : null;
          } catch {
            throw Error("Invallid data structure");
          }

          if (!response.ok) {
            throw Error(data?.error ?? "Failed to upload document");
          }
          if (!data) {
            throw Error("Server returned an empty response");
          }
          return data;
        }),
      )
      .then((data) => {
        const pdf_hash = data.result.stored_pdf.pdf_hash;

        setFile(null);
        navigate(`/update/${pdf_hash}`);
      })
      .catch((err) => {
        setUploading(false);
        setError(err.message);
      });
  }

  return (
    <div className="max-w-5xl mx-auto">
      <NavLink to={"/updatelist"}>
        <button className="bg-white! text-primary! ps-1! my-3 flex flex-row hover:bg-gray-300!">
          <ChevronLeft />
          Back to library
        </button>
      </NavLink>
      <div className="rounded-lg bg-white p-8 shadow-md text-primary max-w-5xl">
        <h2 className="text-xl font-bold">Upload a document</h2>
        <p className="text-xs text-muted-foreground italic">
          Add a PDF file to include as a source.
        </p>
        {file && (
          <div className="my-5">
            <div className="flex flex-row items-center p-2 bg-accent rounded-lg border">
              <div className="rounded-lg bg-primary/10 p-3 me-4">
                <File className="size-8 text-primary" />
              </div>
              <div className="flex flex-col m-2">
                <p className="font-semibold text-xs">Selected Document</p>
                <h3 className="text-muted-foreground text-lg">{file.name}</h3>
              </div>
              <button
                className="ml-auto bg-primary-foreground! hover:bg-gray-300! border-primary! text-primary!"
                disabled={uploading}
                onClick={() => {
                  setFile(null);
                  setError(null);
                }}
              >
                Remove
              </button>
            </div>
          </div>
        )}
        <div
          className="border-2 mt-4 rounded-2xl border-primary border-dashed bg-muted"
          onClick={() => inputRef.current?.click()}
        >
          <input
            type="file"
            ref={inputRef}
            onChange={handleInputChange}
            className="hidden"
          ></input>
          <div className="min-w-2xl py-10 flex flex-col items-center">
            <div className="my-2 rounded-full bg-primary/10 p-3">
              <Upload className="size-7 text-primary" />
            </div>
            <h3>Click to browse</h3>
            <p className="text-xs text-muted-foreground italic">
              PDF only, up to 64MB per file
            </p>
          </div>
        </div>
        <div className="pt-4">
          <button
            className="w-full flex items-center justify-center gap-2"
            disabled={!file || uploading}
            onClick={() => {
              if (file) {
                handleFileUpload(file);
              }
            }}
          >
            {uploading && <Loader2 className="size-4 animate-spin" />}
            {uploading ? "Uploading..." : "Upload Document"}
          </button>
          {error && (
            <p className="text-red-500 flex justify-center py-2 text-sm">
              {error}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
