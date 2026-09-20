import { ChevronRight } from "lucide-react";

export default function CustomTable({
  headers,
  documents,
  onClickHandler,
  setCurrentFile,
}) {
  return (
    <div className="w-full max-w-7xl">
      <table className="w-full min-w-3xl border-collapse text-sm">
        <thead className="sticky top-0 bg-white">
          <tr className="border-b border-border text-muted-foreground">
            {headers.map((header) => (
              <th
                key={header}
                className="px-3 py-2 text-left text-xs font-semibold uppercase tracking-wide"
              >
                {header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="text-black">
          {documents.map((document, index) => (
            <tr
              key={index}
              className="border-b border-border last:border-0 hover:bg-muted/50"
            >
              <td className="px-3 py-2 font-medium">{document.title}</td>
              <td className="px-3 py-2 ">{document.published_at?.split(/[ T]/)[0]}</td>
              <td className="px-3 py-2 ">
                <button
                  className="bg-accent! border-primary! hover:bg-gray-200!"
                  onClick={() =>
                    onClickHandler(document.pdf_hash, setCurrentFile)
                  }
                >
                  <ChevronRight className="text-primary size-4" />
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
