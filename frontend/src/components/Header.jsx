import { Leaf } from "lucide-react";

export default function Header() {
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
    <div className="bg-primary text-white px-2 grid grid-cols-3 items-center">
      <h1 className="font-bold">Scepa Upload Interface</h1>
      <ul className="flex justify-center gap-x-2 ">
        {tabs.map((tab) => {
          return (
            <li
              className="font-semibold hover:cursor-pointer p-1"
              key={tab.title}
            >
              <button className="p-1! ">{tab.title}</button>
            </li>
          );
        })}
      </ul>
      <button className="p-1! justify-self-end">Log in</button>
    </div>
  );
}
