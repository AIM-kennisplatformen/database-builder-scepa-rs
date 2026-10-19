import { ChevronRight, ChevronsUpDown } from "lucide-react";

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
                key={header.title}
                className="relative px-3 py-2 text-left text-xs font-semibold uppercase tracking-wide"
              >
                <button
                  className="-ml-3 flex flex-row gap-2 items-center px-3! py-2! bg-green-100! text-green-900! hover:bg-green-200!"
                  onClick={header.onClick}
                >
                  {header.title}
                  {header.isFilterable ? (
                    <ChevronsUpDown className="size-4" />
                  ) : (
                    header.icon
                  )}
                </button>
                {header.menu && (
                  <div className="absolute left-0 top-full z-20 mt-1 normal-case tracking-normal">
                    {header.menu}
                  </div>
                )}
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
              <td className="px-3 py-2 font-medium max-w-md">
                {document.title ? document.title : "[Missing title]"}
              </td>
              <td className="px-3 py-2 ">
                {document.published_at
                  ? document.published_at?.split(/[ T]/)[0]
                  : "[Missing date]"}
              </td>
              <td className="px-3 py-2 ">
                {document.requiresFixing ? (
                  <div className="flex flex-row">
                    <span className="rounded bg-red-100 px-2 py-0.5 text-xs font-semibold text-red-700">
                      Requires fixing
                    </span>
                  </div>
                ) : (
                  <span className="rounded bg-green-100 px-2 py-0.5 text-xs font-semibold text-green-700">
                    Published
                  </span>
                )}
              </td>
              <td className="px-3 py-2 ">
                <button
                  className="bg-accent! border-primary! hover:bg-gray-200!"
                  onClick={() => onClickHandler(document, setCurrentFile)}
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
