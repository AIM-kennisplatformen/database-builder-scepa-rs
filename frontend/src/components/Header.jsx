import { NavLink } from "react-router-dom";
import { CircleUserRound } from "lucide-react";

export default function Header() {
  const tabs = [
    {
      title: "Upload Document",
      to: "/upload",
    },
    {
      title: "Update Document",
      to: "/update",
    },
    {
      title: "Fix Document",
      to: "/fix",
    },
  ];

  return (
    <div className="bg-primary text-white px-2 py-1.5 grid grid-cols-3 items-center">
      <h1 className="font-bold">Scepa Upload Interface</h1>
      <ul className="flex justify-center gap-x-8">
        {tabs.map((tab) => (
          <li
            className="relative font-semibold hover:cursor-pointer"
            key={tab.title}
          >
            <NavLink
              to={tab.to}
              className={({ isActive }) =>
                `block py-1! px-2! rounded-md transition-colors duration-200 hover:bg-(--primary-dark) ${isActive ? "rounded-b-none!" : ""}`
              }
            >
              {({ isActive }) => (
                <>
                  {tab.title}
                  {isActive && (
                    <span className="absolute inset-x-0 bottom-0 h-0.5 bg-white/80" />
                  )}
                </>
              )}
            </NavLink>
          </li>
        ))}
      </ul>
      <div className="flex items-center gap-x-2 justify-self-end">
        <button className="p-1! text-primary">Log in</button>
      </div>
    </div>
  );
}
