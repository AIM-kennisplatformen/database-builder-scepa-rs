import { Navigate, Route, Routes } from "react-router-dom";
import Header from "./components/Header";
import UploadDocumentPage from "./pages/UploadDocumentPage";
import UpdateDocumentListPage from "./pages/UpdateDocumentListPage";
import UpdateDocumentPage from "./pages/UpdateDocumentPage";
import FixDocumentPage from "./pages/FixDocumentPage";

function App() {
  return (
    <div className="h-screen overflow-hidden flex flex-col">
      <div className="m-0">
        <Header />
      </div>
      <div className="flex flex-1 min-h-0 items-center justify-center bg-slate-100 overflow-hidden">
        <Routes>
          <Route path="/" element={<Navigate to="/upload" replace />} />
          <Route path="/upload" element={<UploadDocumentPage />} />
          <Route path="/updatelist" element={<UpdateDocumentListPage />} />
          <Route path="/update/:pdf_hash" element={<UpdateDocumentPage />} />
          <Route path="/fix" element={<FixDocumentPage />} />
        </Routes>
      </div>
    </div>
  );
}

export default App;
