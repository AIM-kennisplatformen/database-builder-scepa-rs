import { useEffect, useRef } from "react";

const STATUS_OPTIONS = [
  {
    value: "published",
    label: "Published",
    badgeClassName: "bg-green-100 text-green-700",
  },
  {
    value: "requiresFixing",
    label: "Requires fixing",
    badgeClassName: "bg-red-100 text-red-700",
  },
];

export default function DocumentStatusSortMenu({
  selected,
  onChange,
  onClose,
}) {
  const menuRef = useRef(null);

  useEffect(() => {
    function handleMouseDown(event) {
      const headerButton = menuRef.current
        ?.closest("th")
        ?.querySelector(":scope > button");
      const isInside =
        menuRef.current?.contains(event.target) ||
        headerButton?.contains(event.target);
      if (!isInside) onClose();
    }
    document.addEventListener("mousedown", handleMouseDown);
    return () => document.removeEventListener("mousedown", handleMouseDown);
  }, [onClose]);

  const current = selected ?? [];
  const isAllSelected =
    selected === null || current.length === STATUS_OPTIONS.length;

  function toggle(value) {
    const next = current.includes(value)
      ? current.filter((item) => item !== value)
      : [...current, value];
    onChange(next.length === 0 ? null : next);
  }

  return (
    <div
      ref={menuRef}
      className="w-56 animate-menu-in rounded-lg border border-gray-200 bg-white py-2 shadow-lg"
    >
      <p className="px-4 pb-2 text-xs font-medium text-gray-500">
        Show documents with status
      </p>
      <label className="flex cursor-pointer items-center gap-3 px-4 py-2 hover:bg-gray-50">
        <input
          type="checkbox"
          className="size-4 accent-green-800"
          checked={isAllSelected}
          onChange={() => onChange(null)}
        />
        <span className="text-sm font-semibold text-gray-900">
          All statuses
        </span>
      </label>
      <hr className="my-1 border-gray-200" />
      {STATUS_OPTIONS.map((option) => (
        <label
          key={option.value}
          className="flex cursor-pointer items-center gap-3 px-4 py-2 hover:bg-gray-50"
        >
          <input
            type="checkbox"
            className="size-4 accent-green-800"
            checked={current.includes(option.value)}
            onChange={() => toggle(option.value)}
          />
          <span
            className={`rounded px-2 py-0.5 text-xs font-semibold ${option.badgeClassName}`}
          >
            {option.label}
          </span>
        </label>
      ))}
      <hr className="my-1 border-gray-200" />
      <button
        type="button"
        className="w-full px-4! py-2! text-left text-sm font-medium text-gray-600! bg-transparent! hover:bg-gray-50! disabled:cursor-default disabled:opacity-40"
        disabled={selected === null}
        onClick={() => onChange(null)}
      >
        Reset
      </button>
    </div>
  );
}
