import { DragEvent, PointerEvent as ReactPointerEvent, ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { GlobalWorkerOptions, PDFDocumentProxy, PDFPageProxy, getDocument } from "pdfjs-dist";
import pdfWorker from "pdfjs-dist/build/pdf.worker.min.mjs?url";

GlobalWorkerOptions.workerSrc = pdfWorker;

type Role = "author" | "editor";
type Contributor = {
  name: string;
  forename?: string | null;
  surname?: string | null;
  affiliation?: string | null;
  role: Role;
};
type Identifier = {
  kind: string | { other: string };
  value: string;
  scope: string;
};
type Bibliography = {
  title?: string | null;
  authors: Contributor[];
  identifiers: Identifier[];
  publication_date?: string | null;
  publication_year?: number | null;
  publisher?: string | null;
  journal?: string | null;
  journal_abbreviation?: string | null;
  abstract_text: AbstractPassage[];
};
type BoundingBox = {
  page?: number | null;
  x: number;
  y: number;
  width: number;
  height: number;
};
type Passage = {
  type: "text" | "formula";
  id: string;
  text: string;
  coordinates: BoundingBox[];
  heading_context?: string | null;
  section?: string | null;
  references?: unknown[];
  label?: string | null;
};
type AbstractPassage = Omit<Passage, "type">;
type ReviewPassage = Passage & {
  selectionId: string;
  source: "abstract" | "body";
};
type TeiDocument = {
  bibliography: Bibliography;
  body_text: Passage[];
  figures_and_tables: unknown[];
  references: unknown[];
};
type ManualBibliography = {
  title?: string;
  authors?: Contributor[];
  identifiers?: Identifier[];
  publication_date?: string;
  publication_year?: number;
  publisher?: string;
  journal?: string;
  journal_abbreviation?: string;
  abstract_text?: AbstractPassage[];
};
type ManualDocument = { bibliography: ManualBibliography; body_text?: Passage[] };
type Draft = {
  grobid_extraction_data: TeiDocument;
  manual_data: ManualDocument;
};
type UploadResult = {
  result: {
    stored_pdf: { pdf_hash: string; size_bytes: number };
    draft: Draft;
    canonical: unknown;
    warnings: unknown[];
  };
};
type WorkflowKind = "new" | "update" | "repair";
type PublishedDocumentSummary = {
  pdf_hash: string;
  title?: string | null;
  identifiers: Identifier[];
  published_at: string;
};
type PublishedDocument = {
  pdf_hash: string;
  artifact: Draft;
  published_at: string;
};
type ReviewCase = {
  id: number;
  workflow_id: string;
  pdf_hash?: string | null;
  service: string;
  phase: string;
  retryable: boolean;
  error_message: string;
  artifact_content_type: string;
  artifact_size: number;
  status: string;
  created_at: string;
};
type RepairDraft = { case: ReviewCase; draft: Draft & { pdf_hash: string } };

const API = import.meta.env.VITE_API_URL || "/api";

function Glyph({ name, size = 18 }: { name: string; size?: number }) {
  const paths: Record<string, ReactNode> = {
    file: <><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6M8 13h8M8 17h6"/></>,
    upload: <><path d="M12 16V4M7 9l5-5 5 5"/><path d="M20 15v5H4v-5"/></>,
    refresh: <><path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/></>,
    tool: <><path d="M14.7 6.3a4 4 0 0 0-5-5L12 3.6 8.6 7 6.3 4.7a4 4 0 0 0 5 5l-7.6 7.6a2 2 0 0 0 2.8 2.8l7.6-7.6a4 4 0 0 0 5-5L16.8 10 13.4 6.6z"/></>,
    check: <path d="m5 12 4 4L19 6"/>,
    arrow: <><path d="M5 12h14M13 6l6 6-6 6"/></>,
    plus: <path d="M12 5v14M5 12h14"/>,
    trash: <><path d="M4 7h16M9 7V4h6v3M7 7l1 13h8l1-13"/></>,
    database: <><ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v7c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 12v7c0 1.7 3.6 3 8 3s8-1.3 8-3v-7"/></>,
    chevron: <path d="m9 18 6-6-6-6"/>,
    sparkle: <><path d="m12 3 1.2 3.8L17 8l-3.8 1.2L12 13l-1.2-3.8L7 8l3.8-1.2z"/><path d="m19 14 .7 2.3L22 17l-2.3.7L19 20l-.7-2.3L16 17l2.3-.7z"/></>,
  };
  return <svg className="glyph" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>{paths[name]}</svg>;
}

const workflows = [
  { key: "new" as const, icon: "file", title: "New document", detail: "Automatic publication", enabled: true },
  { key: "update" as const, icon: "refresh", title: "Update document", detail: "Published documents", enabled: true },
  { key: "repair" as const, icon: "tool", title: "Fix failed ingestion", detail: "Staged documents", enabled: true },
];

function Sidebar({ selected, onSelect }: { selected: WorkflowKind; onSelect: (workflow: WorkflowKind) => void }) {
  return <aside className="sidebar">
    <div className="brand"><div className="brand-mark"><span>S</span></div><div><strong>SCEPA</strong><small>Knowledge operations</small></div></div>
    <div className="side-label">Workflows</div>
    <nav>{workflows.map((flow) => <button className={`workflow ${flow.enabled && flow.key === selected ? "active" : ""} ${!flow.enabled ? "disabled" : ""}`} key={flow.title} disabled={!flow.enabled} onClick={() => flow.enabled && onSelect(flow.key)}>
      <span className="workflow-icon"><Glyph name={flow.icon}/></span><span><strong>{flow.title}</strong><small>{flow.detail}</small></span>{flow.enabled ? <Glyph name="chevron" size={16}/> : <span className="later">Later</span>}
    </button>)}</nav>
    <div className="sidebar-note"><Glyph name="database"/><div><strong>Canonical graph</strong><span>Manual values always take precedence over extracted metadata.</span></div></div>
    <div className="operator"><span className="avatar">SV</span><div><strong>Simon V.</strong><small>Document operator</small></div><span className="online"/></div>
  </aside>;
}

function Steps({ current, workflow }: { current: number; workflow: WorkflowKind }) {
  const labels = workflow === "new" ? ["Upload PDF", "Review extraction", "Publish"] : workflow === "update" ? ["Choose document", "Review information", "Publish update"] : ["Choose staged document", "Enter required data", "Save fixed document"];
  return <div className="steps">{labels.map((label, index) => <div className={`step ${index < current ? "done" : index === current ? "current" : ""}`} key={label}>
    <span>{index < current ? <Glyph name="check" size={14}/> : index + 1}</span><strong>{label}</strong>{index < 2 && <i/>}
  </div>)}</div>;
}

function RepairTable({ cases, loading, error, onSelect }: { cases: ReviewCase[]; loading: boolean; error: string; onSelect: (reviewCase: ReviewCase) => void }) {
  return <div className="document-list-card">
    <div className="eyebrow"><Glyph name="tool" size={15}/> FIX STAGED DOCUMENT</div>
    <h1>Select a document requiring review</h1>
    <p className="lede">These documents could not finish automatic ingestion. Supply the data required by the canonical graph to publish them.</p>
    {error && <div className="error-box">{error}</div>}
    {loading ? <div className="document-list-state"><span className="spinner"/><span>Loading staged documents…</span></div> : cases.length === 0 ? <div className="document-list-state"><Glyph name="check" size={28}/><strong>No documents require fixing</strong><span>The review queue is clear.</span></div> : <div className="document-table-wrap"><table className="document-table repair-table"><thead><tr><th>Failure</th><th>Pipeline stage</th><th>Staged</th><th/></tr></thead><tbody>{cases.map((reviewCase) => <tr className={!reviewCase.pdf_hash ? "unavailable" : ""} key={reviewCase.id} onClick={() => reviewCase.pdf_hash && onSelect(reviewCase)}><td><strong>{reviewCase.error_message}</strong><small>{reviewCase.pdf_hash ? `${reviewCase.pdf_hash.slice(0, 12)}…` : "Source PDF unavailable"}</small></td><td><span className="failure-stage">{reviewCase.service} · {reviewCase.phase.replaceAll("_", " ")}</span></td><td>{new Date(reviewCase.created_at).toLocaleDateString()}</td><td><button disabled={!reviewCase.pdf_hash} aria-label="Fix staged document"><Glyph name="chevron" size={16}/></button></td></tr>)}</tbody></table></div>}
  </div>;
}

const identifierKind = (identifier: Identifier) => typeof identifier.kind === "string" ? identifier.kind : identifier.kind.other;

function DocumentTable({ documents, loading, error, onSelect }: { documents: PublishedDocumentSummary[]; loading: boolean; error: string; onSelect: (document: PublishedDocumentSummary) => void }) {
  return <div className="document-list-card">
    <div className="eyebrow"><Glyph name="refresh" size={15}/> UPDATE DOCUMENT</div>
    <h1>Select a published document</h1>
    <p className="lede">Only documents that successfully passed the pipeline and reached the canonical graph are shown here.</p>
    {error && <div className="error-box">{error}</div>}
    {loading ? <div className="document-list-state"><span className="spinner"/><span>Loading published documents…</span></div> : documents.length === 0 ? <div className="document-list-state"><Glyph name="database" size={28}/><strong>No published documents yet</strong><span>Publish a new document first, then it will be available for updates.</span></div> : <div className="document-table-wrap"><table className="document-table"><thead><tr><th>Title</th><th>Stable identifiers</th><th>Published</th><th/></tr></thead><tbody>{documents.map((document) => <tr key={document.pdf_hash} onClick={() => onSelect(document)}><td><strong>{document.title || "Untitled document"}</strong><small>{document.pdf_hash.slice(0, 12)}…</small></td><td><div className="table-identifiers">{document.identifiers.length ? document.identifiers.map((identifier, index) => <span key={`${identifierKind(identifier)}-${identifier.value}-${index}`}><b>{identifierKind(identifier).toUpperCase()}</b>{identifier.value}</span>) : <em>SHA-256 only</em>}</div></td><td>{new Date(document.published_at).toLocaleDateString()}</td><td><button aria-label={`Update ${document.title || "document"}`}><Glyph name="chevron" size={16}/></button></td></tr>)}</tbody></table></div>}
  </div>;
}

function UploadPanel({ onSelect, busy, error }: { onSelect: (file: File) => void; busy: boolean; error: string }) {
  const [dragging, setDragging] = useState(false);
  const accept = (files: FileList | null) => files?.[0] && onSelect(files[0]);
  const drop = (event: DragEvent) => { event.preventDefault(); setDragging(false); accept(event.dataTransfer.files); };
  return <div className="stage-card upload-stage">
    <div className="eyebrow"><Glyph name="sparkle" size={15}/> NEW DOCUMENT · AUTOMATIC</div>
    <h1>Start with the source document</h1>
    <p className="lede">Upload a PDF. SCEPA extracts and publishes valid documents automatically, then opens the saved artifact for optional updates. Invalid artifacts are retained for repair.</p>
    <label className={`dropzone ${dragging ? "dragging" : ""} ${busy ? "busy" : ""}`} onDragOver={(e) => { e.preventDefault(); setDragging(true); }} onDragLeave={() => setDragging(false)} onDrop={drop}>
      <input type="file" accept="application/pdf,.pdf" disabled={busy} onChange={(e) => accept(e.target.files)}/>
      <span className="upload-icon">{busy ? <span className="spinner"/> : <Glyph name="upload" size={28}/>}</span>
      <strong>{busy ? "Running ingestion pipeline…" : "Drop a PDF here"}</strong>
      <p>{busy ? "Uploading, extracting TEI, and validating the result" : "or click to choose a file from your computer"}</p>
      {!busy && <span className="browse">Choose PDF</span>}
    </label>
    {error && <div className="error-box">{error}</div>}
    <div className="pipeline-hint"><span><i>1</i> Extract &amp; parse</span><Glyph name="arrow" size={15}/><span><i>2</i> Export to TypeDB</span><Glyph name="arrow" size={15}/><span><i>3</i> Save artifact</span></div>
  </div>;
}

const display = (value: unknown) => value === null || value === undefined || value === "" ? "Not found" : String(value);

function PdfPage({ page, passages, activeId, drawing, onSelect, onDraw }: { page: PDFPageProxy; passages: ReviewPassage[]; activeId?: string; drawing: boolean; onSelect: (id: string) => void; onDraw: (box: BoundingBox) => void }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const layerRef = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<{ x: number; y: number; endX: number; endY: number }>();
  const viewport = useMemo(() => page.getViewport({ scale: 1.45 }), [page]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.width = viewport.width;
    canvas.height = viewport.height;
    const task = page.render({ canvas, viewport });
    return () => task.cancel();
  }, [page, viewport]);

  const boxes = passages.filter((passage) => passage.selectionId === activeId).flatMap((passage) => passage.coordinates
    .filter((box) => box.page === page.pageNumber)
    .map((box, index) => ({ passage, box, index })));

  const point = (event: ReactPointerEvent<HTMLDivElement>) => {
    const bounds = layerRef.current!.getBoundingClientRect();
    return { x: Math.max(0, Math.min(bounds.width, event.clientX - bounds.left)), y: Math.max(0, Math.min(bounds.height, event.clientY - bounds.top)) };
  };
  const startDraw = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drawing || event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    const start = point(event);
    setDrag({ ...start, endX: start.x, endY: start.y });
  };
  const moveDraw = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag) return;
    const end = point(event);
    setDrag({ ...drag, endX: end.x, endY: end.y });
  };
  const finishDraw = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag) return;
    const end = point(event);
    const bounds = layerRef.current!.getBoundingClientRect();
    const x = Math.min(drag.x, end.x); const y = Math.min(drag.y, end.y);
    const width = Math.abs(end.x - drag.x); const height = Math.abs(end.y - drag.y);
    setDrag(undefined);
    if (width < 4 || height < 4) return;
    onDraw({ page: page.pageNumber, x: x / bounds.width * (viewport.width / 1.45), y: y / bounds.height * (viewport.height / 1.45), width: width / bounds.width * (viewport.width / 1.45), height: height / bounds.height * (viewport.height / 1.45) });
  };
  const dragStyle = drag && { left: Math.min(drag.x, drag.endX), top: Math.min(drag.y, drag.endY), width: Math.abs(drag.endX - drag.x), height: Math.abs(drag.endY - drag.y) };

  return <div className="pdf-page" id={`pdf-page-${page.pageNumber}`} style={{ aspectRatio: `${viewport.width} / ${viewport.height}` }}>
    <canvas ref={canvasRef}/>
    <div ref={layerRef} className={`highlight-layer ${drawing ? "drawing" : ""}`} aria-label={`Extraction highlights on page ${page.pageNumber}`} onPointerDown={startDraw} onPointerMove={moveDraw} onPointerUp={finishDraw} onPointerCancel={() => setDrag(undefined)}>
      {boxes.map(({ passage, box, index }) => <button
        type="button"
        key={`${passage.selectionId}-${index}`}
        className={`pdf-highlight ${passage.source} ${activeId === passage.selectionId ? "active" : ""}`}
        style={{
          left: `${(box.x / (viewport.width / 1.45)) * 100}%`,
          top: `${(box.y / (viewport.height / 1.45)) * 100}%`,
          width: `${(box.width / (viewport.width / 1.45)) * 100}%`,
          height: `${(box.height / (viewport.height / 1.45)) * 100}%`,
        }}
        aria-label={`Select extracted chunk: ${passage.text.slice(0, 80)}`}
        title={passage.text}
        onClick={(event) => { event.stopPropagation(); if (!drawing) onSelect(passage.selectionId); }}
      />)}
      {dragStyle && <span className="draw-preview" style={dragStyle}/>} 
    </div>
    <span className="page-number">{page.pageNumber}</span>
  </div>;
}

function SourceReview({ fileName, pdfHash, bodyPassages, abstractPassages, edited, onChange, onReset }: { fileName: string; pdfHash: string; bodyPassages: Passage[]; abstractPassages: AbstractPassage[]; edited: boolean; onChange: (abstracts: AbstractPassage[], body: Passage[]) => void; onReset: () => void }) {
  const [pdf, setPdf] = useState<PDFDocumentProxy>();
  const [pages, setPages] = useState<PDFPageProxy[]>([]);
  const [activeId, setActiveId] = useState<string>();
  const [drawingFor, setDrawingFor] = useState<string>();
  const [error, setError] = useState("");
  const viewerRef = useRef<HTMLDivElement>(null);
  const passages = useMemo<ReviewPassage[]>(() => [
    ...abstractPassages.map((passage) => ({ ...passage, type: "text" as const, source: "abstract" as const, selectionId: `abstract:${passage.id}` })),
    ...bodyPassages.map((passage) => ({ ...passage, source: "body" as const, selectionId: `body:${passage.id}` })),
  ], [abstractPassages, bodyPassages]);

  useEffect(() => {
    let disposed = false;
    setPdf(undefined); setPages([]); setError("");
    const task = getDocument({ url: `${API}/pdfs/${encodeURIComponent(pdfHash)}` });
    task.promise.then(async (document) => {
      const loadedPages = await Promise.all(Array.from({ length: document.numPages }, (_, index) => document.getPage(index + 1)));
      if (!disposed) { setPdf(document); setPages(loadedPages); }
    }).catch((cause) => { if (!disposed) setError(cause instanceof Error ? cause.message : "Could not render this PDF"); });
    return () => { disposed = true; task.destroy(); };
  }, [pdfHash]);

  const selectPassage = (passage: ReviewPassage) => {
    setActiveId(passage.selectionId);
    const page = passage.coordinates.find((box) => box.page)?.page;
    if (page) viewerRef.current?.querySelector(`#pdf-page-${page}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  };
  const locatedCount = passages.filter((passage) => passage.coordinates.some((box) => box.page)).length;
  const updatePassage = (passage: ReviewPassage, patch: Partial<Passage>) => {
    if (passage.source === "abstract") onChange(abstractPassages.map((row) => row.id === passage.id ? { ...row, ...patch } : row), bodyPassages);
    else onChange(abstractPassages, bodyPassages.map((row) => row.id === passage.id ? { ...row, ...patch } : row));
  };
  const reclassify = (passage: ReviewPassage) => {
    if (passage.source === "abstract") {
      const moved: Passage = { type: "text", id: passage.id, text: passage.text, coordinates: passage.coordinates, heading_context: passage.heading_context, section: passage.section, references: passage.references || [] };
      onChange(abstractPassages.filter((row) => row.id !== passage.id), [...bodyPassages, moved]);
    } else {
      const moved: AbstractPassage = { id: passage.id, text: passage.text, coordinates: passage.coordinates, heading_context: passage.heading_context, section: passage.section, references: passage.references || [] };
      onChange([...abstractPassages, moved], bodyPassages.filter((row) => row.id !== passage.id));
    }
    setActiveId(undefined);
  };
  const remove = (passage: ReviewPassage) => {
    if (passage.source === "abstract") onChange(abstractPassages.filter((row) => row.id !== passage.id), bodyPassages);
    else onChange(abstractPassages, bodyPassages.filter((row) => row.id !== passage.id));
    setActiveId(undefined); setDrawingFor(undefined);
  };
  const addChunk = () => {
    const id = `manual-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
    onChange(abstractPassages, [...bodyPassages, { type: "text", id, text: "", coordinates: [], references: [], heading_context: null, section: null }]);
    setActiveId(`body:${id}`); setDrawingFor(`body:${id}`);
  };
  const drawCoordinate = (box: BoundingBox) => {
    const passage = passages.find((row) => row.selectionId === drawingFor);
    if (!passage) return;
    updatePassage(passage, { coordinates: [...passage.coordinates, box] });
    setDrawingFor(undefined);
  };

  return <section className="source-review">
    <div className="source-review-heading">
      <div><span className="source-tag">Source evidence</span><h2>PDF & reviewed chunks</h2><p>Edit text, switch abstract/body classification, or draw a source location on the PDF.</p></div>
      <div className="highlight-key"><span><i className="abstract"/> Abstract</span><span><i className="body"/> Body</span><em>{locatedCount} of {passages.length} located</em></div>
    </div>
    <div className="source-review-grid">
      <div className="pdf-pane">
        <div className="pane-bar"><span className="pdf-icon compact">PDF</span><strong>{fileName}</strong><span>{pdf ? `${pdf.numPages} pages` : "Loading…"}</span></div>
        <div className="pdf-scroll" ref={viewerRef}>
          {error && <div className="viewer-message error-box">{error}</div>}
          {!error && !pages.length && <div className="viewer-message"><span className="spinner"/><p>Rendering source document…</p></div>}
          {drawingFor && <div className="draw-hint">Drag a rectangle around the chunk on any page · <button type="button" onClick={() => setDrawingFor(undefined)}>Cancel</button></div>}
          {pages.map((page) => <PdfPage key={page.pageNumber} page={page} passages={passages} activeId={activeId} drawing={Boolean(drawingFor)} onDraw={drawCoordinate} onSelect={(id) => {
            setActiveId(id);
            document.getElementById(`chunk-${encodeURIComponent(id)}`)?.scrollIntoView({ behavior: "smooth", block: "nearest" });
          }}/>) }
        </div>
      </div>
      <div className="chunks-pane">
        <div className="pane-bar"><div><strong>Abstracts & chunks</strong><span>{bodyPassages.length} body · {abstractPassages.length} abstract</span></div><div className="pane-actions">{edited && <button type="button" onClick={onReset}>Reset</button>}<button type="button" className="add-chunk" onClick={addChunk}><Glyph name="plus" size={13}/> Add chunk</button></div></div>
        <div className="chunks-scroll">
          {passages.length === 0 && <div className="empty-chunks">No text passages were extracted.</div>}
          {passages.map((passage, index) => <div
            role="button" tabIndex={0}
            id={`chunk-${encodeURIComponent(passage.selectionId)}`}
            className={`chunk-card ${passage.source} ${activeId === passage.selectionId ? "active" : ""}`}
            key={passage.selectionId}
            onClick={() => selectPassage(passage)} onKeyDown={(event) => { if (event.key === "Enter") selectPassage(passage); }}
          >
            <span className="chunk-meta"><b>{passage.source === "abstract" ? `Abstract ${abstractPassages.findIndex((item) => item.id === passage.id) + 1}` : `Chunk ${index - abstractPassages.length + 1}`}</b><span>{passage.source === "abstract" ? "Abstract" : passage.section || passage.heading_context || (passage.type === "formula" ? "Formula" : "Body text")}</span>{passage.coordinates[0]?.page && <em>p. {passage.coordinates[0].page}</em>}</span>
            <textarea aria-label={`Edit ${passage.source} text`} value={passage.text} placeholder="Enter chunk text" onClick={(event) => event.stopPropagation()} onChange={(event) => updatePassage(passage, { text: event.target.value, references: [] })}/>
            <span className="chunk-actions"><button type="button" onClick={(event) => { event.stopPropagation(); reclassify(passage); }}>{passage.source === "abstract" ? "Make body chunk" : "Make abstract"}</button><button type="button" className={drawingFor === passage.selectionId ? "active" : ""} onClick={(event) => { event.stopPropagation(); setActiveId(passage.selectionId); setDrawingFor(passage.selectionId); }}>{passage.coordinates.length ? "Add location" : "Draw location"}</button><button type="button" className="remove" aria-label="Remove passage" onClick={(event) => { event.stopPropagation(); remove(passage); }}><Glyph name="trash" size={13}/></button></span>
          </div>)}
        </div>
      </div>
    </div>
  </section>;
}

function CompareField({ label, extracted, value, onChange, type = "text", placeholder }: { label: string; extracted: unknown; value?: string; onChange: (value: string | undefined) => void; type?: string; placeholder?: string }) {
  const overridden = value !== undefined;
  return <div className={`compare-field ${overridden ? "overridden" : ""}`}>
    <div className="compare-label"><label>{label}</label>{overridden && <button onClick={() => onChange(undefined)} type="button">Use extracted</button>}</div>
    <div className="compare-grid">
      <div className="extracted-value"><span className="source-tag">Grobid</span><p>{display(extracted)}</p>{overridden && <span className="excluded">Excluded from canonical</span>}</div>
      <div className="manual-input"><span className="source-tag manual">Manual override</span><input type={type} value={value ?? ""} placeholder={placeholder || `Uses extracted: ${display(extracted)}`} onChange={(e) => onChange(e.target.value || undefined)}/>{overridden && <span className="included">Will overwrite extraction</span>}</div>
    </div>
  </div>;
}

function Contributors({ extracted, value, onChange }: { extracted: Contributor[]; value?: Contributor[]; onChange: (value?: Contributor[]) => void }) {
  const active = value !== undefined;
  const rows = value || [];
  const update = (index: number, patch: Partial<Contributor>) => onChange(rows.map((row, i) => i === index ? { ...row, ...patch } : row));
  return <section className={`collection ${active ? "overridden" : ""}`}>
    <div className="section-title"><div><h3>Contributors <span>{extracted.length}</span></h3><p>Replacing this section replaces the full extracted contributor list.</p></div>{active ? <button className="text-button" onClick={() => onChange(undefined)}>Use extracted list</button> : <button className="secondary" onClick={() => onChange(extracted.map((row) => ({...row})))}><Glyph name="plus" size={15}/> Edit contributors</button>}</div>
    <div className="collection-grid"><div className="extracted-list"><div className="column-heading"><span className="source-tag">Grobid extraction</span>{active && <span className="excluded">Excluded</span>}</div>{extracted.map((person, i) => <div className="person-row" key={i}><span className="initials">{(person.forename?.[0] || "") + (person.surname?.[0] || person.name[0] || "")}</span><div><strong>{person.name}</strong><small>{person.affiliation || person.role}</small></div></div>)}</div>
      <div className="manual-list"><div className="column-heading"><span className="source-tag manual">Manual override</span>{active && <span className="included">Canonical source</span>}</div>{!active ? <div className="empty-override">No override — extracted contributors will be used</div> : <>{rows.map((person, i) => <div className="edit-person" key={i}><div className="input-row"><input aria-label="Given name" placeholder="Given name" value={person.forename || ""} onChange={(e) => update(i, { forename: e.target.value, name: `${e.target.value} ${person.surname || ""}`.trim() })}/><input aria-label="Family name" placeholder="Family name" value={person.surname || ""} onChange={(e) => update(i, { surname: e.target.value, name: `${person.forename || ""} ${e.target.value}`.trim() })}/><button className="icon-button" onClick={() => onChange(rows.filter((_, n) => n !== i))}><Glyph name="trash" size={16}/></button></div><input aria-label="Affiliation" placeholder="Affiliation (optional)" value={person.affiliation || ""} onChange={(e) => update(i, { affiliation: e.target.value || null })}/></div>)}<button className="add-row" onClick={() => onChange([...rows, { name: "", forename: "", surname: "", affiliation: null, role: "author" }])}><Glyph name="plus" size={15}/> Add contributor</button></>}</div>
    </div>
  </section>;
}

function Identifiers({ extracted, value, onChange }: { extracted: Identifier[]; value?: Identifier[]; onChange: (value?: Identifier[]) => void }) {
  const active = value !== undefined;
  const rows = value || [];
  const kind = (id: Identifier) => typeof id.kind === "string" ? id.kind : id.kind.other;
  const kindValue = (id: Identifier) => typeof id.kind === "string" ? id.kind : "other";
  return <section className={`collection identifiers ${active ? "overridden" : ""}`}><div className="section-title"><div><h3>Identifiers <span>{extracted.length}</span></h3><p>A DOI, ISBN, or another stable ID is required for canonical publication.</p></div>{active ? <button className="text-button" onClick={() => onChange(undefined)}>Use extracted list</button> : <button className="secondary" onClick={() => onChange(extracted.map((row) => ({...row})))}><Glyph name="plus" size={15}/> Edit identifiers</button>}</div>
    <div className="collection-grid"><div className="extracted-list"><div className="column-heading"><span className="source-tag">Grobid extraction</span>{active && <span className="excluded">Excluded</span>}</div>{extracted.length ? extracted.map((id, i) => <div className="identifier-row" key={i}><span>{kind(id).toUpperCase()}</span><code>{id.value}</code></div>) : <div className="missing">No identifiers extracted</div>}</div>
      <div className="manual-list"><div className="column-heading"><span className="source-tag manual">Manual override</span>{active && <span className="included">Canonical source</span>}</div>{!active ? <div className="empty-override">No override — extracted identifiers will be used</div> : <>{rows.map((id, i) => <div className="identifier-edit" key={i}><select aria-label="Identifier type" value={kindValue(id)} onChange={(e) => onChange(rows.map((row, n) => n === i ? {...row, kind: e.target.value === "other" ? {other: "manual"} : e.target.value} : row))}><option value="doi">DOI</option><option value="isbn">ISBN</option><option value="pmid">PMID</option><option value="pmc">PMC</option><option value="arxiv">arXiv</option><option value="other">Other</option></select><input aria-label="Identifier value" value={id.value} placeholder="Identifier value" onChange={(e) => onChange(rows.map((row, n) => n === i ? {...row, value: e.target.value} : row))}/><button className="icon-button" onClick={() => onChange(rows.filter((_, n) => n !== i))}><Glyph name="trash" size={16}/></button></div>)}<button className="add-row" onClick={() => onChange([...rows, { kind: "doi", value: "", scope: "document" }])}><Glyph name="plus" size={15}/> Add identifier</button></>}</div></div>
  </section>;
}

function Review({ draft, hash, file, workflow, repairCaseId, onPublished, onReset }: { draft: Draft; hash: string; file?: File; workflow: WorkflowKind; repairCaseId?: number; onPublished: (canonical: unknown) => void; onReset: () => void }) {
  const extracted = draft.grobid_extraction_data;
  const b = extracted.bibliography;
  const [manual, setManual] = useState<ManualBibliography>(draft.manual_data.bibliography || {});
  const [manualBody, setManualBody] = useState<Passage[] | undefined>(draft.manual_data.body_text);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const set = <K extends keyof ManualBibliography>(key: K, value: ManualBibliography[K]) => setManual((current) => { const next = {...current}; if (value === undefined) delete next[key]; else next[key] = value; return next; });
  const passageEdited = manual.abstract_text !== undefined || manualBody !== undefined;
  const abstractPassages = manual.abstract_text ?? b.abstract_text ?? [];
  const bodyPassages = manualBody ?? extracted.body_text;
  const overrideCount = Object.keys(manual).length + (manualBody === undefined ? 0 : 1);
  const effectiveTitle = (manual.title !== undefined ? manual.title : b.title)?.trim();
  const effectiveAuthors = manual.authors !== undefined ? manual.authors : b.authors;
  const invalidReason = !effectiveTitle ? "A title is required." : !effectiveAuthors?.length ? "At least one contributor is required." : effectiveAuthors.some((author) => !(author.name || author.forename || author.surname)?.trim()) ? "Every contributor needs a name." : "";
  const artifact = useMemo(() => ({ pdf_hash: hash, grobid_extraction_data: extracted, manual_data: { bibliography: manual, ...(manualBody === undefined ? {} : { body_text: manualBody }) } }), [hash, extracted, manual, manualBody]);
  const setPassages = (abstracts: AbstractPassage[], body: Passage[]) => { set("abstract_text", abstracts); setManualBody(body); };
  const resetPassages = () => { set("abstract_text", undefined); setManualBody(undefined); };
  const publish = async () => {
    if (invalidReason) { setError(invalidReason); return; }
    setSaving(true); setError("");
    try {
      const manualData = { bibliography: manual, ...(manualBody === undefined ? {} : { body_text: manualBody }) };
      const endpoint = workflow === "new" ? `${API}/drafts/${hash}` : workflow === "update" ? `${API}/documents/${hash}` : `${API}/documents/requiring-fixing/${repairCaseId}`;
      const body = workflow === "repair" ? { manual_data: manualData, enrich: false } : manualData;
      const response = await fetch(endpoint, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
      if (!response.ok) throw new Error((await response.text()) || `Publish failed (${response.status})`);
      const result = await response.json(); onPublished(result.canonical);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not publish the document"); }
    finally { setSaving(false); }
  };
  const isRepair = workflow === "repair";
  return <div className="review-page">
    <div className="review-head"><div><button className="back" onClick={onReset}>← {workflow === "new" ? "Start over" : workflow === "update" ? "Back to documents" : "Back to staged documents"}</button><div className="eyebrow"><Glyph name={isRepair ? "tool" : "sparkle"} size={15}/> {workflow === "new" ? "EXTRACTION COMPLETE" : workflow === "update" ? "PUBLISHED DOCUMENT" : "MANUAL REPAIR"}</div><h1>{workflow === "new" ? "Review document metadata" : workflow === "update" ? "Update document information" : "Enter required document data"}</h1><p>{isRepair ? "Complete at least the title and one named contributor. The PDF hash supplies a stable fallback identifier." : "Check extracted and manual information, then change only what needs updating."}</p></div><div className="document-chip"><span className="pdf-icon">PDF</span><div><strong>{file?.name || b.title || (isRepair ? "Staged document.pdf" : "Published document.pdf")}</strong><small>{file ? `${(file.size / 1024 / 1024).toFixed(1)} MB` : hash.slice(0, 12)} · {workflow === "new" ? "Extraction complete" : workflow === "update" ? "Published" : "Requires fixing"}</small></div><span className={isRepair ? "failure-dot" : "success-dot"}>{isRepair ? <Glyph name="tool" size={14}/> : <Glyph name="check" size={14}/>}</span></div></div>
    <SourceReview fileName={file?.name || "Uploaded document.pdf"} pdfHash={hash} bodyPassages={bodyPassages} abstractPassages={abstractPassages} edited={passageEdited} onChange={setPassages} onReset={resetPassages}/>
    <div className="rule-banner"><div className="rule-icon"><Glyph name="arrow"/></div><div><strong>Manual values have priority</strong><p>The extracted value always stays visible. When you add an override, the extracted value is marked as excluded from canonical data.</p></div><span>{overrideCount} override{overrideCount === 1 ? "" : "s"}</span></div>
    <div className="review-card"><div className="card-heading"><div><span>01</span><div><h2>Publication details</h2><p>Core metadata used to identify the document.</p></div></div><div className="legend"><span><i className="blue"/> Extracted</span><span><i className="orange"/> Manual</span></div></div>
      <CompareField label="Title" extracted={b.title} value={manual.title} onChange={(v) => set("title", v)}/>
      <div className="field-pair"><CompareField label="Publication date" extracted={b.publication_date} value={manual.publication_date} onChange={(v) => set("publication_date", v)}/><CompareField label="Publication year" extracted={b.publication_year} value={manual.publication_year?.toString()} type="number" onChange={(v) => set("publication_year", v ? Number(v) : undefined)}/></div>
      <div className="field-pair"><CompareField label="Journal" extracted={b.journal} value={manual.journal} onChange={(v) => set("journal", v)}/><CompareField label="Publisher" extracted={b.publisher} value={manual.publisher} onChange={(v) => set("publisher", v)}/></div>
    </div>
    <div className="review-card"><div className="card-heading"><div><span>02</span><div><h2>People & identifiers</h2><p>Canonical identity and contribution records.</p></div></div></div><Contributors extracted={b.authors || []} value={manual.authors} onChange={(v) => set("authors", v)}/><Identifiers extracted={b.identifiers || []} value={manual.identifiers} onChange={(v) => set("identifiers", v)}/></div>
    <div className="review-card extraction-summary"><div className="card-heading"><div><span>03</span><div><h2>Extraction retained</h2><p>The original extraction stays attached while reviewed passages are stored as overrides.</p></div></div></div><div className="stats"><div><strong>{bodyPassages.length}</strong><span>Reviewed body passages</span></div><div><strong>{abstractPassages.length}</strong><span>Reviewed abstracts</span></div><div><strong>{extracted.figures_and_tables.length}</strong><span>Figures & tables</span></div></div><details><summary>Preview stored artifact</summary><pre>{JSON.stringify(artifact, null, 2)}</pre></details></div>
    {isRepair && <div className="review-card enrichment-card"><div><span className="workflow-icon"><Glyph name="sparkle"/></span><div><h2>External enrichment</h2><p>Enrichment with external APIs is planned and is not yet available.</p></div></div><label><input type="checkbox" disabled/> Enrich before saving <span>Planned</span></label></div>}
    {error && <div className="error-box sticky-error">{error}</div>}
    <div className="publish-bar"><div><Glyph name="database"/><p><strong>{invalidReason || `Ready to ${workflow === "new" ? "publish" : workflow === "update" ? "update" : "save fixed document"}`}</strong><span>{invalidReason ? "Complete the required fields before continuing" : overrideCount ? `${overrideCount} manual override${overrideCount === 1 ? "" : "s"} will be applied` : "All extracted values will be used"}</span></p></div><button className="primary" disabled={saving || Boolean(invalidReason)} title={invalidReason || undefined} onClick={publish}>{saving ? <span className="spinner small"/> : <Glyph name="arrow"/>}{saving ? "Publishing…" : workflow === "new" ? "Publish to canonical graph" : workflow === "update" ? "Update canonical graph" : "Save fixed document"}</button></div>
  </div>;
}

function Complete({ canonical, workflow, onReset }: { canonical: unknown; workflow: WorkflowKind; onReset: () => void }) {
  return <div className="stage-card complete"><span className="complete-icon"><Glyph name="check" size={34}/></span><div className="eyebrow">{workflow === "new" ? "PUBLISHED" : workflow === "update" ? "UPDATED" : "FIXED"} SUCCESSFULLY</div><h1>{workflow === "new" ? "Document added to the graph" : workflow === "update" ? "Document updated in the graph" : "Staged document published"}</h1><p className="lede">{workflow === "new" ? "The canonical model was built from the extraction plus your manual overrides." : workflow === "update" ? "Only changed graph sections were deleted from the old artifact and inserted from the new artifact." : "The manual data passed validation, reached the canonical graph, and the review case was resolved."}</p><div className="complete-actions"><button className="primary" onClick={onReset}><Glyph name={workflow === "new" ? "plus" : workflow === "update" ? "refresh" : "tool"}/> {workflow === "new" ? "Ingest another document" : workflow === "update" ? "Update another document" : "Fix another document"}</button></div><details><summary>View canonical response</summary><pre>{JSON.stringify(canonical, null, 2)}</pre></details></div>;
}

export default function App() {
  const [workflow, setWorkflow] = useState<WorkflowKind>("new");
  const [stage, setStage] = useState<"upload" | "list" | "review" | "complete">("upload");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [documents, setDocuments] = useState<PublishedDocumentSummary[]>([]);
  const [reviewCases, setReviewCases] = useState<ReviewCase[]>([]);
  const [repairCaseId, setRepairCaseId] = useState<number>();
  const [file, setFile] = useState<File>();
  const [draft, setDraft] = useState<Draft>();
  const [hash, setHash] = useState("");
  const [canonical, setCanonical] = useState<unknown>();
  const reset = () => { setStage(workflow === "new" ? "upload" : "list"); setFile(undefined); setDraft(undefined); setHash(""); setRepairCaseId(undefined); setError(""); setCanonical(undefined); };
  const selectWorkflow = (next: WorkflowKind) => {
    setWorkflow(next); setStage(next === "new" ? "upload" : "list"); setFile(undefined); setDraft(undefined); setHash(""); setRepairCaseId(undefined); setError(""); setCanonical(undefined);
  };
  useEffect(() => {
    if (workflow !== "update" || stage !== "list") return;
    let disposed = false;
    setBusy(true); setError("");
    fetch(`${API}/documents`).then(async (response) => {
      if (!response.ok) throw new Error((await response.text()) || `Could not load documents (${response.status})`);
      return response.json() as Promise<PublishedDocumentSummary[]>;
    }).then((rows) => { if (!disposed) setDocuments(rows); }).catch((cause) => { if (!disposed) setError(cause instanceof Error ? cause.message : "Could not load published documents"); }).finally(() => { if (!disposed) setBusy(false); });
    return () => { disposed = true; };
  }, [workflow, stage]);
  useEffect(() => {
    if (workflow !== "repair" || stage !== "list") return;
    let disposed = false;
    setBusy(true);
    fetch(`${API}/documents/requiring-fixing`).then(async (response) => {
      if (!response.ok) throw new Error((await response.text()) || `Could not load staged documents (${response.status})`);
      return response.json() as Promise<ReviewCase[]>;
    }).then((rows) => { if (!disposed) setReviewCases(rows); }).catch((cause) => { if (!disposed) setError(cause instanceof Error ? cause.message : "Could not load staged documents"); }).finally(() => { if (!disposed) setBusy(false); });
    return () => { disposed = true; };
  }, [workflow, stage]);
  const openDocument = async (document: PublishedDocumentSummary) => {
    setBusy(true); setError("");
    try {
      const response = await fetch(`${API}/documents/${encodeURIComponent(document.pdf_hash)}`);
      if (!response.ok) throw new Error((await response.text()) || `Could not load document (${response.status})`);
      const payload: PublishedDocument = await response.json();
      setDraft(payload.artifact); setHash(payload.pdf_hash); setStage("review");
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not load the document"); }
    finally { setBusy(false); }
  };
  const openRepairCase = async (reviewCase: ReviewCase) => {
    setBusy(true); setError("");
    try {
      const response = await fetch(`${API}/documents/requiring-fixing/${reviewCase.id}`);
      if (!response.ok) throw new Error((await response.text()) || `Could not load staged document (${response.status})`);
      const payload: RepairDraft = await response.json();
      setDraft(payload.draft); setHash(payload.draft.pdf_hash); setRepairCaseId(payload.case.id); setStage("review");
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not load the staged document"); }
    finally { setBusy(false); }
  };
  const upload = async (selected: File) => {
    if (selected.type !== "application/pdf" && !selected.name.toLowerCase().endsWith(".pdf")) { setError("Please choose a PDF document."); return; }
    setFile(selected); setBusy(true); setError("");
    let published = false;
    try {
      const response = await fetch(`${API}/pdfs`, { method: "POST", headers: { "content-type": "application/pdf" }, body: selected });
      if (!response.ok) throw new Error((await response.text()) || `Ingestion failed (${response.status})`);
      const payload: UploadResult = await response.json();
      published = true;
      const artifactResponse = await fetch(`${API}/documents/${encodeURIComponent(payload.result.stored_pdf.pdf_hash)}`);
      if (!artifactResponse.ok) throw new Error((await artifactResponse.text()) || `Could not retrieve the saved artifact (${artifactResponse.status})`);
      const saved: PublishedDocument = await artifactResponse.json();
      setWorkflow("update"); setHash(saved.pdf_hash); setDraft(saved.artifact); setCanonical(payload.result.canonical); setStage("review");
    } catch (cause) {
      setWorkflow(published ? "update" : "repair"); setStage("list");
      setError(`${published ? "The document was published, but its saved artifact could not be retrieved." : "The new-document workflow failed and retained the document for repair."} ${cause instanceof Error ? cause.message : ""}`.trim());
    }
    finally { setBusy(false); }
  };
  const current = stage === "upload" || stage === "list" ? 0 : stage === "review" ? 1 : 3;
  return <div className="app-shell"><Sidebar selected={workflow} onSelect={selectWorkflow}/><main><header className="topbar"><div><span>Document operations</span><i>/</i><strong>{workflow === "new" ? "New document" : workflow === "update" ? "Update document" : "Fix staged document"}</strong></div><span className="environment"><i/> Pipeline online</span></header><div className="workspace"><Steps current={current} workflow={workflow}/>{stage === "upload" && <UploadPanel onSelect={upload} busy={busy} error={error}/>} {stage === "list" && workflow === "update" && <DocumentTable documents={documents} loading={busy} error={error} onSelect={openDocument}/>} {stage === "list" && workflow === "repair" && <RepairTable cases={reviewCases} loading={busy} error={error} onSelect={openRepairCase}/>} {stage === "review" && draft && <Review draft={draft} hash={hash} file={file} workflow={workflow} repairCaseId={repairCaseId} onReset={reset} onPublished={(model) => { setCanonical(model); setStage("complete"); }}/>} {stage === "complete" && <Complete canonical={canonical} workflow={workflow} onReset={reset}/>}</div></main></div>;
}
