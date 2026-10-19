import { Search } from "lucide-react";

export default function SearchBox({ setSearch }) {
  return (
    <div className="flex flex-row flex-1 ">
      <div className="flex flex-1 flex-col">
        <div className="relative w-full max-w-xl bg-gray-100 rounded-sm focus-within:ring-2 focus-within:ring-inset focus-within:ring-primary/40">
          <Search className="pointer-events-none absolute left-2 top-1/2 size-5 -translate-y-1/2" />
          <input
            className="w-full p-1 pl-9 text-sm outline-none"
            type="text"
            id=""
            placeholder="Search documents..."
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
      </div>
    </div>
  );
}
