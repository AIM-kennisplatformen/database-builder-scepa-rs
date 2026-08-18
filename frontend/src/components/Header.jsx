import { useState } from "react";
import { CircleUserRound } from "lucide-react";

export default function Header() {
  const [activeTab, setActiveTab] = useState(0);

  const tabs = [
    {
      title: "Upload Document",
    },
    {
      title: "Update Document",
    },
    {
      title: "Fix Document",
    },
  ];

  return (
    <div className="bg-primary text-white px-2 py-1.5 grid grid-cols-3 items-center">
      <h1 className="font-bold">Scepa Upload Interface</h1>
      <ul className="flex justify-center gap-x-8">
        {tabs.map((tab, index) => {
          const isActive = index === activeTab;
          return (
            <li
              className="relative font-semibold hover:cursor-pointer"
              key={tab.title}
            >
              <button
                className={`py-1!  ${isActive ? "rounded-b-none!" : ""} `}
                onClick={() => setActiveTab(index)}
              >
                {tab.title}
              </button>
              {isActive && (
                <span className="absolute inset-x-0 bottom-0 h-0.5 bg-white/80" />
              )}
            </li>
          );
        })}
      </ul>
      <div className="flex items-center gap-x-2 justify-self-end">
        <button className="p-1! text-primary">Log in</button>
      </div>
    </div>
  );
}
