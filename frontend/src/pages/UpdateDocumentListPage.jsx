import { SavePlus, Upload } from "lucide-react";
import CustomTable from "../components/CustomTable";
import { useEffect, useState } from "react";
import { NavLink, useNavigate } from "react-router-dom";
import { toast, ToastContainer } from "react-toastify";
import SearchBox from "../components/SearchBox";
import DocumentStatusSortMenu from "../components/DocumentStatusSortMenu";

export default function UpdateDocumentList({}) {
  const [documents, setDocuments] = useState(null);
  const [search, setSearch] = useState("");
  const [alphabeticSort, setAlphabeticSort] = useState(true);
  const [isStatusMenuOpen, setIsStatusMenuOpen] = useState(false);
  const [statusFilter, setStatusFilter] = useState(null);

  const navigate = useNavigate();

  const tableHeaders = [
    {
      title: "Title",
      isFilterable: true,
      onClick: () => setAlphabeticSort(!alphabeticSort),
    },
    {
      title: "Published",
      isIconDown: true,
      onClick: null,
    },
    {
      title: "Status",
      isIconDown: !isStatusMenuOpen,
      onClick: () => setIsStatusMenuOpen(!isStatusMenuOpen),
      menu: isStatusMenuOpen ? (
        <DocumentStatusSortMenu
          selected={statusFilter}
          onChange={setStatusFilter}
          onClose={() => setIsStatusMenuOpen(false)}
        />
      ) : null,
    },
  ];

  useEffect(() => {
    loadDocuments();
  }, []);

  function tableOnClickHandler(document) {
    // Fixing documents are addressed by review case id, normal ones by pdf_hash.
    if (document.requiresFixing) {
      navigate(`/update/${document.id}?requiresFixing=true`);
    } else if (document.pdf_hash) {
      navigate(`/update/${document.pdf_hash}`);
    }
  }

  async function fetchDocument(url) {
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Failed to load documents`);
    }
    return res.json();
  }

  async function loadDocuments() {
    try {
      const normal = await fetchDocument("/api/documents");
      const failed = await fetchDocument("/api/documents/requiring-fixing");

      setDocuments([
        ...failed.map((doc) => ({ ...doc, requiresFixing: true })),
        ...normal.map((doc) => ({ ...doc, requiresFixing: false })),
      ]);
    } catch (err) {
      toast.error(err.message);
    }
  }

  const visibleDocuments = (documents ?? [])
    .filter((document) =>
      (document.title ?? "").toLowerCase().includes(search.toLowerCase()),
    )
    .filter(
      (document) =>
        statusFilter === null ||
        statusFilter.includes(
          document.requiresFixing ? "requiresFixing" : "published",
        ),
    )
    .sort((a, b) => {
      if (!a.title && !b.title) return 0;
      if (!a.title) return 1;
      if (!b.title) return -1;
      const result = a.title.localeCompare(b.title);
      return alphabeticSort ? result : -result;
    });

  return (
    <div className="flex flex-col w-full max-w-7xl mx-auto justify-center">
      <div className="flex justify-end my-3">
        <NavLink to={"/upload"}>
          <button className="ps-3! flex flex-row gap-2">
            <Upload className="size-5" />
            Upload a document
          </button>
        </NavLink>
      </div>
      <div className="rounded-lg bg-white p-8 shadow-md text-primary flex-col max-h-full overflow-hidden">
        <div className="flex flex-row gap-1.5 mb-2 justify-center ">
          <SavePlus className="size-4" />
          <h2 className="text-xs font-semibold">Update documents</h2>
        </div>
        <h1 className="text-lg text-black font-bold text-center">
          Select a document
        </h1>
        <p className="text-xs text-muted-foreground italic mt-2 text-center">
          All uploaded documents are shown here.
        </p>
        <div className="mt-4 w-full h-96 overflow-auto scrollbar-custom">
          {documents && documents.length > 0 && (
            <>
              <div className="my-2 p-2 flex flex-row justify-between border-t ">
                <SearchBox setSearch={setSearch} />
                <p className="text-sm text-gray-600">
                  Showing{" "}
                  <span className="font-bold text-black">
                    {visibleDocuments.length}
                  </span>{" "}
                  from the {documents.length} documents
                </p>
              </div>
              <CustomTable
                headers={tableHeaders}
                documents={visibleDocuments}
                onClickHandler={tableOnClickHandler}
              />
            </>
          )}
          {documents && documents.length === 0 && (
            <p className="text-lg text-primary justify-center flex">
              No uploaded documents found
            </p>
          )}
        </div>
        <ToastContainer />
      </div>
    </div>
  );
}
