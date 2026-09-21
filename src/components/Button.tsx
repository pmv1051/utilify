import type { ButtonHTMLAttributes } from "react";
import type { QuotaScope } from "../lib/api";
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
 * that call's quota is exhausted the button disables itself with an
 * explanation. `scope` says which family of endpoints it uses, since Spotify
 * runs out of one while the others still answer; playlist calls are the
 * common case. Pass `local` for buttons that never touch Spotify
 * (navigation, cancel, settings, updates, stats).
 */
export function Button({
  variant = "primary",
  className = "",
  local = false,
  scope = "playlists",
  disabled,
  title,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  local?: boolean;
  scope?: QuotaScope;
}) {
  const cooldown = useQuotaCooldown(scope);
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
