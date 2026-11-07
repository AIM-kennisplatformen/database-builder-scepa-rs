import { LockKeyhole } from "lucide-react";

export default function NoAccessPage() {
  return (
    <div className="text-primary bg-white rounded mx-auto max-w-xl p-8">
      <div className="flex flex-col items-center text-center">
        <div className="bg-primary/10 rounded-full p-5 w-max">
          <LockKeyhole className="size-7" />
        </div>
        <div className="m-2 mt-4">
          <p className="font-bold">You don't seem to have access...</p>
          <p className="text-sm text-gray-500">
            Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do
            eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim
            ad minim veniam.
          </p>
        </div>
      </div>
    </div>
  );
}
