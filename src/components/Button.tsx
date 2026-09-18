import type { ButtonHTMLAttributes } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger";

const styles: Record<Variant, string> = {
  primary: "bg-spotify text-black hover:bg-spotify-dark font-semibold",
  secondary: "bg-panel-2 text-zinc-100 hover:bg-line border border-line",
  ghost: "text-zinc-300 hover:bg-panel-2",
  danger: "bg-red-600/90 text-white hover:bg-red-600",
};

export function Button({
  variant = "primary",
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  return (
    <button
      {...props}
      className={`inline-flex items-center gap-2 rounded-md px-3.5 py-2 text-sm transition disabled:cursor-not-allowed disabled:opacity-50 ${styles[variant]} ${className}`}
    />
  );
}
