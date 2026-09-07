import { ChevronDown } from "lucide-react";
import { useState } from "react";
import { TEXT_REGEX, isValidField } from "../utils/validation";

const AUTHOR_FIELDS = [
  { key: "forename", label: "Forename", regex: TEXT_REGEX },
  { key: "surname", label: "Surname", regex: TEXT_REGEX },
  { key: "affiliation", label: "Affiliation", regex: TEXT_REGEX },
  { key: "role", label: "Role", regex: TEXT_REGEX },
];

function getInitials(author) {
  return `${author.forename?.[0] ?? ""}${author.surname?.[0] ?? ""}`.toUpperCase();
}

function stripAffiliationNumber(affiliation) {
  // Removes a leading footnote-style number
  // ^\s*  optional leading whitespace
  // \d+   one or more digits (the footnote marker)
  // \s*   optional whitespace after the number
  return affiliation?.replace(/^\s*\d+\s*/, "") ?? affiliation;
}

export default function AuthorDisplay({ author, onChange }) {
  const [open, setOpen] = useState(false);
  const [fieldErrors, setFieldErrors] = useState({});

  function handleChange(field, regex, value) {
    onChange(field, value);
    setFieldErrors((prev) => ({
      ...prev,
      [field]: !isValidField(value, regex),
    }));
  }

  return (
    <div className="border rounded">
      <div
        className="w-full flex flex-row items-center border-b p-2 hover:cursor-pointer"
        onClick={() => setOpen(!open)}
      >
        <div className="flex items-center justify-center w-9 h-9 shrink-0 rounded-full text-white text-sm font-medium bg-primary">
          {getInitials(author)}
        </div>
        <div className="flex flex-col px-2 min-w-0 flex-1">
          <p className="text-black">
            {author.forename} {author.surname}
          </p>
          <p className="text-xs text-muted-foreground truncate">
            {stripAffiliationNumber(author.affiliation)}
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
          {AUTHOR_FIELDS.map((field) => {
            const value =
              field.key === "affiliation"
                ? (stripAffiliationNumber(author.affiliation) ?? "")
                : (author[field.key] ?? "");

            return (
              <div className="flex flex-col py-1" key={field.key}>
                <label className="text-sm text-primary font-medium">
                  {field.label}
                </label>
                <input
                  className={`bg-accent text-black rounded px-2 py-1 border ${
                    fieldErrors[field.key] ? "border-red-500" : "border-border"
                  }`}
                  value={value}
                  onChange={(e) =>
                    handleChange(field.key, field.regex, e.target.value)
                  }
                  aria-invalid={fieldErrors[field.key] || undefined}
                />
                {fieldErrors[field.key] && (
                  <span className="text-xs text-red-500">
                    Invalid format for {field.label.toLowerCase()}.
                  </span>
                )}
              </div>
            );
          })}
          <div className="flex justify-end py-2">
            <button className="bg-red-700!">Delete</button>
          </div>
        </div>
      )}
    </div>
  );
}
