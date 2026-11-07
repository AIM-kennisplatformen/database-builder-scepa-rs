import { useMatch, useNavigate } from "react-router-dom";
import { ArrowLeft, LibraryBig, LogIn } from "lucide-react";

export default function Header() {
  const navigate = useNavigate();
  const isUpdatePage = useMatch("/update/:pdf_hash");

  return (
    <div className="bg-primary text-white py-1.5">
      <div
        className={`px-4 flex flex-row items-center justify-between ${
          isUpdatePage ? "w-full" : "max-w-7xl mx-auto"
        }`}
      >
        <div className="flex flex-row items-center gap-4 ">
          {isUpdatePage && (
            <button
              className="flex flex-row items-center gap-2 bg-white! text-primary! hover:bg-gray-300! py-2! ps-3!"
              onClick={() => navigate("/updatelist")}
            >
              <ArrowLeft />
              Back to library
            </button>
          )}
          <div
            className="flex flex-row items-center gap-2 hover:cursor-pointer"
            onClick={() => navigate("/updatelist")}
          >
            <LibraryBig className="size-9" />
            <div className="leading-tight">
              <h1 className="font-bold text-md">ChatEP</h1>
              <p className="text-xs">Document Library</p>
            </div>
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
