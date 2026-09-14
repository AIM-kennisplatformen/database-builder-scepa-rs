import { useNavigate } from "react-router-dom";

export default function CustomSuccessToast() {
  const navigate = useNavigate();
  return (
    <div className="flex flex-col ">
      <strong>Document saved successfully</strong>
      <div className="flex justify-center">
        <button className="shrink-0 " onClick={() => navigate("/upload")}>
          Upload more
        </button>
      </div>
    </div>
  );
}
