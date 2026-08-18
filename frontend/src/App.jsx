import Header from "./components/Header";
import UploadDocument from "./pages/UploadDocument";

function App() {
  return (
    <div className="">
      <div className="m-0">
        <Header />
      </div>
      <div className="flex min-h-screen items-center justify-center bg-slate-100">
        <UploadDocument />
      </div>
    </div>
  );
}

export default App;
