import { useEffect, useRef, useState } from "react";
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
const VIEWPORT_PADDING = 24;

export default function PdfViewer({ file }) {
  const [numPages, setNumPages] = useState(null);
  const [pageNumber, setPageNumber] = useState(1);
  const [scale, setScale] = useState(1);
  const [pageWidth, setPageWidth] = useState(null);
  const scrollRef = useRef(null);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const updateWidth = () => {
      setPageWidth(Math.max(el.clientWidth - VIEWPORT_PADDING * 2, 0));
    };

    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

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
    <div className="flex h-full w-full flex-col items-center gap-3">
      <div className="relative h-full w-full overflow-hidden rounded border border-border bg-muted/20">
        <div ref={scrollRef} className="h-full w-full overflow-auto p-6">
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
            {pageWidth ? (
              <Page
                pageNumber={pageNumber}
                width={pageWidth * scale}
              />
            ) : null}
          </Document>
        </div>

        <div className="pointer-events-none absolute inset-x-0 bottom-2 flex justify-center">
          <div className="pointer-events-auto flex items-center gap-2 rounded-full border border-border bg-white/90 px-3 py-1.5 text-sm shadow-md backdrop-blur">
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
            {/* <button
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
            </button> */}
          </div>
        </div>
      </div>
    </div>
  );
}
