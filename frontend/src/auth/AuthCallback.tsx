import { useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { exchangeCallback } from "@/api/auth";
import { useAuth } from "./AuthProvider";
import { LoadingSpinner } from "@/components/LoadingSpinner";

/**
 * Handles the OIDC callback redirect.
 *
 * Reads `code` and `state` from the URL query params, exchanges them
 * for a session via the backend, stores the CSRF token, and redirects
 * to the app root.
 */
export function AuthCallback() {
  const { setAuthenticated } = useAuth();
  const navigate = useNavigate();
  const [error, setError] = useState<string | null>(null);
  const exchanged = useRef(false);

  useEffect(() => {
    // Guard against double-execution in StrictMode
    if (exchanged.current) return;
    exchanged.current = true;

    async function handleCallback() {
      const params = new URLSearchParams(window.location.search);
      const code = params.get("code");
      const state = params.get("state");

      // Check for provider-side errors
      const errorParam = params.get("error");
      if (errorParam) {
        const description =
          params.get("error_description") ?? "Authentication was denied";
        setError(description);
        return;
      }

      if (!code || !state) {
        setError("Missing authorization code or state parameter");
        return;
      }

      try {
        const response = await exchangeCallback(code, state);
        setAuthenticated(
          { user_id: response.user_id, csrf_token: response.csrf_token },
          response.csrf_token,
        );
        void navigate({ to: "/" });
      } catch (e) {
        setError(
          e instanceof Error ? e.message : "Failed to complete authentication",
        );
      }
    }

    void handleCallback();
  }, [setAuthenticated, navigate]);

  if (error) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-gray-50 px-4">
        <div className="w-full max-w-sm space-y-6 text-center">
          <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-red-100">
            <svg
              className="h-6 w-6 text-red-600"
              fill="none"
              viewBox="0 0 24 24"
              strokeWidth="1.5"
              stroke="currentColor"
              aria-hidden="true"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M12 9v3.75m9-.75a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9 3.75h.008v.008H12v-.008Z"
              />
            </svg>
          </div>
          <div>
            <h1 className="text-xl font-semibold text-gray-900">
              Unable to sign in
            </h1>
            <p className="mt-2 text-sm text-gray-600">
              Something went wrong during sign in. This can happen if the link
              expired or was already used. Please try again.
            </p>
          </div>
          <div className="flex flex-col gap-3">
            <a
              href="/login"
              className="inline-block rounded-md bg-blue-600 px-4 py-2.5 text-sm font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
            >
              Try again
            </a>
            <details className="text-left">
              <summary className="cursor-pointer text-xs text-gray-400 hover:text-gray-600">
                Technical details
              </summary>
              <p className="mt-1 rounded bg-gray-100 p-2 text-xs text-gray-500">
                {error}
              </p>
            </details>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-gray-50">
      <div className="text-center">
        <LoadingSpinner className="mx-auto" />
        <p className="mt-4 text-sm text-gray-600">Completing sign in...</p>
      </div>
    </div>
  );
}
