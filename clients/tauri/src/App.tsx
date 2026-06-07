import { APP_NAME } from "./config";

// Placeholder shell — replaced by the auth gate + chat view in the next steps.
export default function App() {
  return (
    <div className="flex h-full items-center justify-center">
      <div className="text-center">
        <h1 className="bg-gradient-to-r from-brand-400 to-brand-600 bg-clip-text text-4xl font-bold text-transparent">
          {APP_NAME}
        </h1>
        <p className="mt-2 text-sm text-slate-500">scaffolding…</p>
      </div>
    </div>
  );
}
