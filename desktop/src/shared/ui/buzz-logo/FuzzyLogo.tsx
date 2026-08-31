import { cn } from "@/shared/lib/cn";
import { BuzzMark } from "./BuzzMark";

export type FuzzyLogoProps = {
  /** When false, skips the looping feTurbulence texture filter and uses a CSS pulse instead. */
  fuzz?: boolean;
  className?: string;
  ariaLabel?: string;
  loop?: boolean;
  /** When looping, hide the mark for this many seconds between plays. */
  loopRestSeconds?: number;
  /** Set false when a parent drives its own opacity animation over the mark. */
  pulse?: boolean;
  reverse?: boolean;
  variant?: string;
};

/**
 * The fuzzy Buzz mark. v8 ships a built-in animated texture (looping fractal-noise
 * turbulence + grain) applied via an SVG filter. Set `fuzz={false}` to render the
 * crisp geometry with a lightweight CSS pulse — recommended for long-lived mounts.
 */
export function FuzzyLogo({
  fuzz: _fuzz = true,
  className,
  ariaLabel = "Buzz logo",
  loop: _loop = false,
  loopRestSeconds: _loopRestSeconds = 0,
  pulse = true,
  reverse: _reverse = false,
  variant: _variant = "v8",
}: FuzzyLogoProps) {
  return (
    <span aria-label={ariaLabel} role="img">
      <BuzzMark className={cn(pulse && "buzz-logo--pulse", className)} />
    </span>
  );
}
