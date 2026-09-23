import { useNavigate } from "react-router-dom";

export default function CustomSuccessToast() {
  const navigate = useNavigate();
  return (
    <div className="flex flex-col w-full">
      <h3 className={`text-sm font-semibold text-zinc-800`}>
        Document saved successfully!
      </h3>
      <div className="flex items-center justify-between">
        <p className="text-sm">Your changes have been saved</p>
        <button
          className="shrink-0 mt-2! p-1.5!"
          onClick={() => navigate("/upload")}
        >
          Upload more
        </button>
      </div>
    </div>
  );
}
