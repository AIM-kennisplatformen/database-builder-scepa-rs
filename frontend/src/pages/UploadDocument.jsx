import { Upload } from "lucide-react";
import { useRef, useState } from "react";

export default function UploadDocument() {
  const [file, setFile] = useState(null);
  const inputRef = useRef(null);

  const handleInputChange = (e) => {
    const file = e.target.files?.[0];
    if (file) {
      setFile(file);
    }
  };

  return (
    <div className="rounded-lg bg-white p-8 shadow-md">
      <h2 className="text-lg">Upload documents</h2>
      <p className="text-xs text-muted-foreground">
        Add PDF files to include as sources.
      </p>
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
        <div className="p-20 flex flex-col items-center">
          <div className="my-2 rounded-full bg-primary/10 p-3">
            <Upload className="size-7 text-primary" />
          </div>
          <h3>Drag & drop PDFs, or click to browse</h3>
          <p className="text-xs text-muted-foreground">
            PDF only, up to 20MB per file
          </p>
        </div>
      </div>
      <div className="pt-4">
        <button className="w-full">Upload Document</button>
      </div>
    </div>
  );
}
