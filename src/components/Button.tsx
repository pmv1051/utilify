import type { ButtonHTMLAttributes } from "react";
import { useQuotaCooldown } from "../lib/quota";

type Variant = "primary" | "secondary" | "ghost" | "danger";

const styles: Record<Variant, string> = {
  primary: "bg-spotify text-black hover:bg-spotify-dark font-semibold",
  secondary: "bg-panel-2 text-zinc-100 hover:bg-line border border-line",
  ghost: "text-zinc-300 hover:bg-panel-2",
  danger: "bg-red-600/90 text-white hover:bg-red-600",
};

/**
 * App button. Almost every button here ends in a Spotify API call, so while
 * the API quota is exhausted buttons disable themselves with an explanation.
 * Pass `local` for buttons that never touch Spotify (navigation, cancel,
 * settings, updates).
 */
export function Button({
  variant = "primary",
  className = "",
  local = false,
  disabled,
  title,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; local?: boolean }) {
  const cooldown = useQuotaCooldown();
  const paused = !local && cooldown.active;
  return (
    <button
      {...props}
      disabled={disabled || paused}
      title={paused ? cooldown.reason : title}
      className={`inline-flex items-center gap-2 rounded-md px-3.5 py-2 text-sm transition disabled:cursor-not-allowed disabled:opacity-50 ${styles[variant]} ${className}`}
    />
  );
}
