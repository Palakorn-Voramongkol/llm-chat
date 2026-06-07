import { Loader2 } from "lucide-react";
import { AuthProvider, useAuth } from "./auth/useAuth";
import { LoginScreen } from "./auth/LoginScreen";
import { AuthorizationGate } from "./auth/AuthorizationGate";
import { ChatView } from "./chat/ChatView";

function Shell() {
  const { loading, identity } = useAuth();
  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="animate-spin text-brand-500" size={28} />
      </div>
    );
  }
  if (!identity) return <LoginScreen />;
  return (
    <AuthorizationGate>
      <ChatView />
    </AuthorizationGate>
  );
}

export default function App() {
  return (
    <AuthProvider>
      <Shell />
    </AuthProvider>
  );
}
