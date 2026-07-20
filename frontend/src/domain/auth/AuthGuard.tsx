import type { ReactNode } from "react";
import { useAuth } from "./AuthProvider";
import { LoginPage } from "./LoginPage";
import { LoadingSpinner } from "@/common/components/LoadingSpinner";

interface AuthGuardProps {
  children: ReactNode;
}

/**
 * Wraps protected content and shows the login page if unauthenticated.
 * Shows a loading spinner while the session is being verified.
 */
export function AuthGuard({ children }: AuthGuardProps) {
  const { auth } = useAuth();

  if (auth.status === "loading") {
    return (
      <div className="flex min-h-screen items-center justify-center bg-gray-50">
        <LoadingSpinner />
      </div>
    );
  }

  if (auth.status === "unauthenticated") {
    return <LoginPage />;
  }

  return <>{children}</>;
}
