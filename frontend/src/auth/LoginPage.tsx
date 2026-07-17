import { useCallback, useState } from "react";
import { useAuth } from "./AuthProvider";

export function LoginPage() {
  const { login } = useAuth();
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleLogin = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      await login();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to start login");
      setIsLoading(false);
    }
  }, [login]);

  return (
    <div className="flex min-h-screen items-center justify-center bg-gray-50 px-4">
      <div className="w-full max-w-sm space-y-6 text-center">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">Haiker</h1>
          <p className="mt-2 text-sm text-gray-600">
            Sign in to manage your hiking activities
          </p>
        </div>

        {error && (
          <div
            role="alert"
            className="rounded-md bg-red-50 px-4 py-3 text-sm text-red-700"
          >
            {error}
          </div>
        )}

        <button
          type="button"
          onClick={handleLogin}
          disabled={isLoading}
          className="w-full rounded-md bg-blue-600 px-4 py-2.5 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {isLoading ? "Redirecting..." : "Sign in"}
        </button>
      </div>
    </div>
  );
}
