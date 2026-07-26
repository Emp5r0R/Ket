import { Check, ChevronDown } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ProtocolPreference } from "../lib/protocols";

export interface ProtocolMenuOption {
  value: ProtocolPreference;
  label: string;
}

interface ProtocolMenuProps {
  id: string;
  value: ProtocolPreference;
  options: ProtocolMenuOption[];
  disabled: boolean;
  onChange: (value: ProtocolPreference) => void;
}

export function ProtocolMenu({
  id,
  value,
  options,
  disabled,
  onChange,
}: ProtocolMenuProps) {
  const [open, setOpen] = useState(false);
  const shellRef = useRef<HTMLDivElement>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = Math.max(0, options.findIndex((option) => option.value === value));
  const selected = options[selectedIndex];
  const listId = `${id}-options`;

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  useEffect(() => {
    if (!open) return undefined;
    const closeOnOutsidePress = (event: PointerEvent) => {
      if (!shellRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePress);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePress);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  const openAndFocus = (index: number) => {
    setOpen(true);
    window.requestAnimationFrame(() => optionRefs.current[index]?.focus());
  };

  const choose = (next: ProtocolPreference) => {
    onChange(next);
    setOpen(false);
    window.requestAnimationFrame(() => document.getElementById(id)?.focus());
  };

  const moveOptionFocus = (event: React.KeyboardEvent, offset: number) => {
    event.preventDefault();
    const current = optionRefs.current.findIndex((option) => option === document.activeElement);
    const next = (Math.max(0, current) + offset + options.length) % options.length;
    optionRefs.current[next]?.focus();
  };

  return (
    <div
      className={`protocol-menu-shell ${open ? "is-open" : ""}`}
      ref={shellRef}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setOpen(false);
      }}
    >
      <button
        id={id}
        type="button"
        className="protocol-menu-trigger"
        role="combobox"
        aria-controls={listId}
        aria-expanded={open}
        aria-haspopup="listbox"
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            openAndFocus(selectedIndex);
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            openAndFocus(selectedIndex);
          }
        }}
      >
        <span>{selected?.label ?? "Automatic"}</span>
        <ChevronDown size={16} aria-hidden="true" />
      </button>
      {open ? (
        <div
          id={listId}
          className="protocol-menu-options"
          role="listbox"
          aria-label="Available protocols"
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") moveOptionFocus(event, 1);
            if (event.key === "ArrowUp") moveOptionFocus(event, -1);
            if (event.key === "Home") {
              event.preventDefault();
              optionRefs.current[0]?.focus();
            }
            if (event.key === "End") {
              event.preventDefault();
              optionRefs.current.at(-1)?.focus();
            }
          }}
        >
          {options.map((option, index) => (
            <button
              key={option.value}
              ref={(element) => { optionRefs.current[index] = element; }}
              type="button"
              role="option"
              aria-selected={option.value === value}
              className={option.value === value ? "is-selected" : ""}
              onClick={() => choose(option.value)}
            >
              <span>{option.label}</span>
              {option.value === value ? <Check size={15} aria-hidden="true" /> : null}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
