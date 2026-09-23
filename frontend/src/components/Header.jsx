import { useNavigate } from "react-router-dom";
import { LibraryBig, LogIn } from "lucide-react";

export default function Header() {
  const navigate = useNavigate();

  return (
    <div className="bg-primary text-white py-1.5">
      <div className="max-w-7xl mx-auto px-4 flex flex-row items-center justify-between">
        <div
          className="flex flex-row items-center gap-2 ps-4 hover:cursor-pointer"
          onClick={() => navigate("/updatelist")}
        >
          <LibraryBig className="size-9" />
          <div className="leading-tight">
            <h1 className="font-bold text-md">ChatEP</h1>
            <p className="text-xs">Document Library</p>
          </div>
        </div>
        <button className="flex flex-row gap-2 ps-3! bg-white! text-primary! hover:bg-gray-300! py-2!">
          <LogIn />
          Login
        </button>
      </div>
    </div>
  );
}
