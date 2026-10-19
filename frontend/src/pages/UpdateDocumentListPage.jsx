import {
  ArrowDownAZ,
  ArrowUpZA,
  CalendarArrowDown,
  CalendarArrowUp,
  Funnel,
  SavePlus,
  Upload,
  X,
} from "lucide-react";
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
  const [chronologicalSort, setChronologicalSort] = useState(false);
  // The most recently clicked column decides what the table is sorted on.
  const [sortKey, setSortKey] = useState("title");
  const [isStatusMenuOpen, setIsStatusMenuOpen] = useState(false);
  const [statusFilter, setStatusFilter] = useState(null);

  const navigate = useNavigate();

  const tableHeaders = [
    {
      title: "Title",
      isFilterable: sortKey !== "title",
      icon: alphabeticSort ? (
        <ArrowDownAZ className="size-4" />
      ) : (
        <ArrowUpZA className="size-4" />
      ),
      onClick: () => {
        setSortKey("title");
        setAlphabeticSort(!alphabeticSort);
      },
    },
    {
      title: "Published",
      // Show the neutral icon while another column is the active sort.
      isFilterable: sortKey !== "published_at",
      icon: chronologicalSort ? (
        <CalendarArrowUp className="size-4" />
      ) : (
        <CalendarArrowDown className="size-4" />
      ),
      onClick: () => {
        setSortKey("published_at");
        setChronologicalSort(!chronologicalSort);
      },
    },
    {
      title: "Status",
      icon: isStatusMenuOpen ? (
        <X className="size-4" />
      ) : statusFilter == null ? (
        <Funnel className="size-4" />
      ) : (
        <Funnel className="size-4 fill-current" />
      ),
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
      // Documents with a missing value always sort last.
      if (!a[sortKey] && !b[sortKey]) return 0;
      if (!a[sortKey]) return 1;
      if (!b[sortKey]) return -1;

      if (sortKey === "title") {
        const result = a.title.localeCompare(b.title);
        return alphabeticSort ? result : -result;
      }

      const result = new Date(a.published_at) - new Date(b.published_at);
      return chronologicalSort ? result : -result;
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
