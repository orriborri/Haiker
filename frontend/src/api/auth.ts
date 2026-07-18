import { z } from "zod";
import { getCsrfToken, clearCsrfToken } from "./client";

// --- Schemas ---

const LoginResponseSchema = z.object({
  authorization_url: z.string(),
});

const CallbackResponseSchema = z.object({
  csrf_token: z.string(),
  user_id: z.string(),
});

const SessionUserSchema = z.object({
  user_id: z.string(),
  csrf_token: z.string(),
});

// --- Types ---

export type LoginResponse = z.infer<typeof LoginResponseSchema>;
export type CallbackResponse = z.infer<typeof CallbackResponseSchema>;
export type SessionUser = z.infer<typeof SessionUserSchema>;

// --- API Functions ---

/**
 * Initiate the OIDC login flow.
 * Returns an authorization URL the client should redirect the browser to.
 */
export async function login(): Promise<LoginResponse> {
  const response = await fetch("/auth/login", {
    method: "POST",
    credentials: "include",
  });

  if (!response.ok) {
    throw new Error(`Login request failed: ${response.status}`);
  }

  const json: unknown = await response.json();
  return LoginResponseSchema.parse(json);
}

/**
 * Exchange the OIDC callback parameters for a session.
 * The server sets the session cookie and returns CSRF token + user ID.
 */
export async function exchangeCallback(
  code: string,
  state: string,
): Promise<CallbackResponse> {
  const params = new URLSearchParams({ code, state });
  const response = await fetch(`/auth/callback?${params.toString()}`, {
    method: "GET",
    credentials: "include",
    headers: {
      "Accept": "application/json",
    },
  });

  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    const detail = (body as Record<string, unknown>).detail ?? response.statusText;
    throw new Error(`Authentication failed: ${detail}`);
  }

  const json: unknown = await response.json();
  return CallbackResponseSchema.parse(json);
}

/**
 * Log out the current user by revoking the session.
 * Clears the session cookie and local CSRF token.
 */
export async function logout(): Promise<void> {
  const csrfToken = getCsrfToken();
  const headers: Record<string, string> = {};
  if (csrfToken) {
    headers["x-csrf-token"] = csrfToken;
  }

  await fetch("/auth/logout", {
    method: "POST",
    credentials: "include",
    headers,
  });

  clearCsrfToken();
}

/**
 * Check if the current session is valid by calling /me.
 * Returns the user info if authenticated, null if not.
 */
export async function checkSession(): Promise<SessionUser | null> {
  try {
    const response = await fetch("/me", {
      method: "GET",
      credentials: "include",
    });

    if (response.status === 401) {
      return null;
    }

    if (!response.ok) {
      return null;
    }

    const json: unknown = await response.json();
    return SessionUserSchema.parse(json);
  } catch {
    return null;
  }
}
