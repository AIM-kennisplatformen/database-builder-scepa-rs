import { File, FileText, Loader2, Upload, X } from "lucide-react";
import { useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

export default function UploadDocumentPage() {
  const [file, setFile] = useState(null);
  const inputRef = useRef(null);
  const [error, setError] = useState("");
  const [uploading, setUploading] = useState(false);
  const navigate = useNavigate();

  const handleInputChange = (e) => {
    const file = e.target.files?.[0];
    if (file && file.type == "application/pdf") {
      setFile(file);
      setError("");
    } else {
      setError("Document must be a PDF");
    }
  };

  function handleFileUpload(file) {
    setUploading(true);
    setError("");
    fetch("/api/pdfs", {
      method: "POST",
      headers: { "Content-Type": "application/pdf" },
      body: file,
    })
      .then((res) => {
        if (!res.ok) {
          return res.text().then((text) => {
            console.log(text);

            throw new Error(text || `Upload failed (${res.status})`);
          });
        }
        return res.json();
      })
      .then(() => {
        setFile(null);
        navigate("/update");
      })
      .catch((err) => setError(err.message))
      .finally(() => setUploading(false));
  }

  return (
    <div className="rounded-lg bg-white p-8 shadow-md text-primary max-w-5xl">
      <h2 className="text-xl font-bold">Upload documents</h2>
      <p className="text-xs text-muted-foreground italic">
        Add PDF files to include as sources.
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
              onClick={() => {
                setFile(null);
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
  );
}
