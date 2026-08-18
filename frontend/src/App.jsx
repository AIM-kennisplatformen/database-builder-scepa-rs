import { Navigate, Route, Routes } from "react-router-dom";
import Header from "./components/Header";
import UploadDocument from "./pages/UploadDocument";
import UpdateDocument from "./pages/UpdateDocument";
import FixDocument from "./pages/FixDocument";

function App() {
  return (
    <div className="">
      <div className="m-0">
        <Header />
      </div>
      <div className="flex min-h-screen items-center justify-center bg-slate-100">
        <Routes>
          <Route path="/" element={<Navigate to="/upload" replace />} />
          <Route path="/upload" element={<UploadDocument />} />
          <Route path="/update" element={<UpdateDocument />} />
          <Route path="/fix" element={<FixDocument />} />
        </Routes>
      </div>
    </div>
  );
}

export default App;
