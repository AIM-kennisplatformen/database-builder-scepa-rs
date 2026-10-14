import { useEffect, useRef, useState } from "react";
import { Document, Page, pdfjs } from "react-pdf";
import { ZoomIn, ZoomOut } from "lucide-react";
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
const PAGE_GAP = 16;
const RENDER_MARGIN = "100% 0px"; // render pages up to one viewport away
const DEFAULT_ASPECT_RATIO = 1.414; // A4 until the first page is measured

// Only mounts the (expensive) canvas/text layers while the slot is near the
// viewport; the placeholder keeps the scroll height stable meanwhile.
function PageSlot({ pageNumber, width, height, root }) {
  const slotRef = useRef(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const el = slotRef.current;
    if (!el || !root) return;

    const observer = new IntersectionObserver(
      ([entry]) => setVisible(entry.isIntersecting),
      { root, rootMargin: RENDER_MARGIN },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [root]);

  return (
    <div
      ref={slotRef}
      data-page-number={pageNumber}
      className="flex justify-center"
      style={{ minHeight: height, minWidth: width }}
    >
      {visible ? <Page pageNumber={pageNumber} width={width} /> : null}
    </div>
  );
}

export default function PdfViewer({ file }) {
  const [numPages, setNumPages] = useState(null);
  const [currentPage, setCurrentPage] = useState(1);
  const [scale, setScale] = useState(1);
  const [containerWidth, setContainerWidth] = useState(null);
  const [aspectRatio, setAspectRatio] = useState(DEFAULT_ASPECT_RATIO);
  const [scrollEl, setScrollEl] = useState(null);

  useEffect(() => {
    if (!scrollEl) return;

    const updateWidth = () => {
      setContainerWidth(
        Math.max(scrollEl.clientWidth - VIEWPORT_PADDING * 2, 0),
      );
    };

    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(scrollEl);
    return () => observer.disconnect();
  }, [scrollEl]);

  // Track which page is in the middle of the viewport for the indicator.
  useEffect(() => {
    if (!scrollEl || !numPages) return;

    const updateCurrentPage = () => {
      const middle =
        scrollEl.getBoundingClientRect().top + scrollEl.clientHeight / 2;
      let found = 1;
      for (const slot of scrollEl.querySelectorAll("[data-page-number]")) {
        if (slot.getBoundingClientRect().top > middle) break;
        found = Number(slot.dataset.pageNumber);
      }
      setCurrentPage(found);
    };

    updateCurrentPage();
    scrollEl.addEventListener("scroll", updateCurrentPage, { passive: true });
    return () => scrollEl.removeEventListener("scroll", updateCurrentPage);
  }, [scrollEl, numPages]);

  async function handleLoadSuccess(pdf) {
    setNumPages(pdf.numPages);
    setCurrentPage(1);
    try {
      const firstPage = await pdf.getPage(1);
      const { width, height } = firstPage.getViewport({ scale: 1 });
      setAspectRatio(height / width);
    } catch {
      // keep the default placeholder ratio
    }
  }

  function zoomOut() {
    setScale((current) => Math.max(current - SCALE_STEP, MIN_SCALE));
  }

  function zoomIn() {
    setScale((current) => Math.min(current + SCALE_STEP, MAX_SCALE));
  }

  const pageWidth = containerWidth ? containerWidth * scale : null;

  return (
    <div className="flex h-full w-full flex-col items-center gap-3">
      <div className="relative h-full w-full overflow-hidden rounded border border-border bg-muted/20">
        <div ref={setScrollEl} className="h-full w-full overflow-auto p-6">
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
          >
            {pageWidth && numPages ? (
              <div
                className="mx-auto flex w-max flex-col"
                style={{ gap: PAGE_GAP }}
              >
                {Array.from({ length: numPages }, (_, index) => (
                  <PageSlot
                    key={index + 1}
                    pageNumber={index + 1}
                    width={pageWidth}
                    height={pageWidth * aspectRatio}
                    root={scrollEl}
                  />
                ))}
              </div>
            ) : null}
          </Document>
        </div>

        <div className="pointer-events-none absolute inset-x-0 bottom-2 flex justify-center ">
          <div className="pointer-events-auto flex items-center rounded-2xl border flex-row gap-4 text-primary border-border bg-white/90 px-3 py-1.5 text-sm shadow-md backdrop-blur">
            <span className="text-muted-foreground">
              Page {currentPage} of {numPages ?? "?"}
            </span>
            |
            <button
              onClick={zoomOut}
              disabled={scale <= MIN_SCALE}
              className="rounded hover:bg-muted/50 disabled:opacity-40"
              aria-label="Zoom out"
            >
              <ZoomOut className="size-4" />
            </button>
            {scale * 100}%
            <button
              onClick={zoomIn}
              disabled={scale >= MAX_SCALE}
              className="rounded hover:bg-muted/50 disabled:opacity-40"
              aria-label="Zoom in"
            >
              <ZoomIn className="size-4" />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
