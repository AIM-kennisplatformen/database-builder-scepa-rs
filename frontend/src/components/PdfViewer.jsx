import { useState } from "react";
import { Document, Page, pdfjs } from "react-pdf";
import { ChevronLeft, ChevronRight, ZoomIn, ZoomOut } from "lucide-react";
import "react-pdf/dist/Page/AnnotationLayer.css";
import "react-pdf/dist/Page/TextLayer.css";

pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.min.mjs",
  import.meta.url,
).toString();

const MIN_SCALE = 0.5;
const MAX_SCALE = 2.5;
const SCALE_STEP = 0.25;

export default function PdfViewer({ file }) {
  const [numPages, setNumPages] = useState(null);
  const [pageNumber, setPageNumber] = useState(1);
  const [scale, setScale] = useState(1);

  function handleLoadSuccess({ numPages }) {
    setNumPages(numPages);
    setPageNumber(1);
  }

  function goToPrevPage() {
    setPageNumber((current) => Math.max(current - 1, 1));
  }

  function goToNextPage() {
    setPageNumber((current) => Math.min(current + 1, numPages ?? current));
  }

  function zoomOut() {
    setScale((current) => Math.max(current - SCALE_STEP, MIN_SCALE));
  }

  function zoomIn() {
    setScale((current) => Math.min(current + SCALE_STEP, MAX_SCALE));
  }

  return (
    <div className="flex w-full max-h-96 max-w-3xl flex-col items-center gap-3">
      <div className="flex w-full items-center justify-center z-500 rounded border border-border bg-white px-3 py-2 text-sm">
        <div className="flex items-center gap-2">
          <button
            onClick={goToPrevPage}
            disabled={pageNumber <= 1}
            className="rounded p-1 hover:bg-muted/50 disabled:opacity-40"
          >
            <ChevronLeft className="size-4" />
          </button>
          <span className="text-muted-foreground">
            Page {pageNumber} of {numPages ?? "?"}
          </span>
          <button
            onClick={goToNextPage}
            disabled={!numPages || pageNumber >= numPages}
            className="rounded p-1 hover:bg-muted/50 disabled:opacity-40"
          >
            <ChevronRight className="size-4" />
          </button>
        </div>
        {/* <div className="flex items-center gap-2">
          <button
            onClick={zoomOut}
            disabled={scale <= MIN_SCALE}
            className="rounded p-1 hover:bg-muted/50 disabled:opacity-40"
          >
            <ZoomOut className="size-4" />
          </button>
          <span className="text-muted-foreground w-12 text-center">
            {Math.round(scale * 100)}%
          </span>
          <button
            onClick={zoomIn}
            disabled={scale >= MAX_SCALE}
            className="rounded p-1 hover:bg-muted/50 disabled:opacity-40"
          >
            <ZoomIn className="size-4" />
          </button>
        </div> */}
      </div>

      <div className="w-full overflow-auto rounded border border-border bg-muted/20">
        <Document
          file={file}
          onLoadSuccess={handleLoadSuccess}
          loading={
            <div className="p-6 text-sm text-muted-foreground">
              Loading PDF...
            </div>
          }
          error={
            <div className="p-6 text-sm text-destructive">
              Failed to load PDF.
            </div>
          }
          className="flex justify-center"
        >
          <Page pageNumber={pageNumber} scale={scale} />
        </Document>
      </div>
    </div>
  );
}
