import { ChevronDown } from "lucide-react";
import { useState } from "react";

function getInitials(author) {
  return `${author.forename?.[0] ?? ""}${author.surname?.[0] ?? ""}`.toUpperCase();
}

export default function AuthorDisplay({ author }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="border rounded">
      <div className="w-full flex flex-row items-center border-b p-2">
        <div className="flex items-center justify-center w-9 h-9 shrink-0 rounded-full text-white text-sm font-medium bg-primary">
          {getInitials(author)}
        </div>
        <div className="flex flex-col px-2 min-w-0 flex-1">
          <p className="text-black">
            {author.forename} {author.surname}
          </p>
          <p className="text-xs text-muted-foreground truncate">
            {author.affiliation}
          </p>
        </div>
        <button
          className="p-2! ml-2 bg-accent! border border-primary! text-primary!"
          onClick={() => setOpen(!open)}
        >
          <ChevronDown
            className={`shrink-0 transition-transform duration-200 ${
              open ? "rotate-180" : ""
            }`}
          />
        </button>
      </div>
      {open && (
        <div className="p-2">
          <div className="flex flex-col py-1">
            <label className="text-sm text-primary font-medium">Forename</label>
            <input
              className="bg-accent text-black rounded px-2 py-1"
              defaultValue={author.forename}
            />
          </div>
          <div className="flex flex-col py-1">
            <label className="text-sm text-primary font-medium">Surname</label>
            <input
              className="bg-accent text-black rounded px-2 py-1"
              defaultValue={author.surname}
            />
          </div>
          <div className="flex flex-col py-1">
            <label className="text-sm text-primary font-medium">
              Affiliation
            </label>
            <input
              className="bg-accent text-black rounded px-2 py-1"
              defaultValue={author.affiliation}
            />
          </div>
          <div className="flex flex-col py-1">
            <label className="text-sm text-primary font-medium">Role</label>
            <input
              className="bg-accent text-black rounded px-2 py-1"
              defaultValue={author.role}
            />
          </div>
        </div>
      )}
    </div>
  );
}
