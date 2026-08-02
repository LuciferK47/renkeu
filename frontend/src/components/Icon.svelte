<script>
  // One icon set, drawn on a 16px grid at a single stroke weight.
  //
  // This replaces two things that had been standing in for icons: bracketed
  // words like [NOTE] and [SPARKLE], and emoji. Emoji render in the platform's
  // own colour and style, so they never match the interface around them and
  // look different on every OS; the placeholders were never meant to ship.
  //
  // Everything inherits currentColor and sits on the same grid, so an icon
  // beside a label lines up and takes the label's colour, including the state
  // colours.

  export let name
  export let size = 16
  /** Accessible label. Omit for decorative icons sitting next to real text. */
  export let label = ''
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  stroke="currentColor"
  stroke-width="1.4"
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? 'img' : 'presentation'}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : 'true'}
>
  {#if name === 'document'}
    <path d="M9.2 1.8H4.4A1.2 1.2 0 0 0 3.2 3v10a1.2 1.2 0 0 0 1.2 1.2h7.2A1.2 1.2 0 0 0 12.8 13V5.4z" />
    <path d="M9.2 1.8v3.6h3.6" />
  {:else if name === 'check'}
    <path d="M2.8 8.4l3.4 3.4 7-7.6" />
  {:else if name === 'check-circle'}
    <circle cx="8" cy="8" r="6.2" />
    <path d="M5.3 8.1l1.9 1.9 3.6-3.9" />
  {:else if name === 'alert'}
    <circle cx="8" cy="8" r="6.2" />
    <path d="M8 4.9v3.6M8 11.1h.01" />
  {:else if name === 'warning'}
    <path d="M7 2.4L1.6 12a1.15 1.15 0 0 0 1 1.7h10.8a1.15 1.15 0 0 0 1-1.7L9 2.4a1.15 1.15 0 0 0-2 0z" />
    <path d="M8 6.2v2.6M8 11.4h.01" />
  {:else if name === 'sparkle'}
    <!-- The AI mark. A four-point star reads as "generated" without borrowing
         a vendor's logo or falling back to a robot emoji. -->
    <path d="M8 1.6l1.5 4.1 4.1 1.5-4.1 1.5L8 12.8 6.5 8.7 2.4 7.2l4.1-1.5z" />
    <path d="M12.9 11.4l.5 1.3 1.3.5-1.3.5-.5 1.3-.5-1.3-1.3-.5 1.3-.5z" />
  {:else if name === 'wrench'}
    <path d="M10.1 5.9a3.2 3.2 0 0 1-4-4l1.9 1.9 1.6-.3.3-1.6L8 0a3.2 3.2 0 0 1 4 4z" transform="translate(1.6 1.6)" />
    <path d="M8.7 7.3l4.4 4.4a1.3 1.3 0 0 1-1.8 1.8L6.9 9.1" />
  {:else if name === 'search'}
    <circle cx="7.2" cy="7.2" r="4.6" />
    <path d="M13.4 13.4l-2.8-2.8" />
  {:else if name === 'folder'}
    <path d="M14.2 12.2a1.1 1.1 0 0 1-1.1 1.1H2.9a1.1 1.1 0 0 1-1.1-1.1V3.8a1.1 1.1 0 0 1 1.1-1.1h3.3l1.1 1.7h4.8a1.1 1.1 0 0 1 1.1 1.1z" />
  {:else if name === 'folder-open'}
    <path d="M1.8 12.2V3.8a1.1 1.1 0 0 1 1.1-1.1h3.3l1.1 1.7h4.8a1.1 1.1 0 0 1 1.1 1.1v.9" />
    <path d="M1.8 12.2l1.7-5h11l-1.7 5z" />
  {:else if name === 'message'}
    <path d="M14 9.5a1.3 1.3 0 0 1-1.3 1.3H4.7L2 13.5V3.4a1.3 1.3 0 0 1 1.3-1.3h9.4A1.3 1.3 0 0 1 14 3.4z" />
  {:else if name === 'user'}
    <path d="M13 13.7v-1.3a2.6 2.6 0 0 0-2.6-2.6H5.6A2.6 2.6 0 0 0 3 12.4v1.3" />
    <circle cx="8" cy="5.1" r="2.6" />
  {:else if name === 'eye'}
    <path d="M1 8s2.5-4.7 7-4.7S15 8 15 8s-2.5 4.7-7 4.7S1 8 1 8z" />
    <circle cx="8" cy="8" r="1.9" />
  {:else if name === 'close'}
    <path d="M12 4L4 12M4 4l8 8" />
  {:else if name === 'send'}
    <path d="M8 13.5V3.2" />
    <path d="M3.6 7.6L8 3.2l4.4 4.4" />
  {:else if name === 'bolt'}
    <path d="M8.9 1.4L2.6 9.1h4.5l-.9 5.5 6.3-7.7H8z" />
  {:else if name === 'settings'}
    <circle cx="8" cy="8" r="2.2" />
    <path d="M12.9 10a1.1 1.1 0 0 0 .22 1.21l.04.04a1.33 1.33 0 1 1-1.88 1.88l-.04-.04a1.11 1.11 0 0 0-1.88.79v.11a1.33 1.33 0 1 1-2.66 0v-.06a1.11 1.11 0 0 0-1.94-.74l-.04.04a1.33 1.33 0 1 1-1.88-1.88l.04-.04a1.11 1.11 0 0 0-.79-1.88h-.11a1.33 1.33 0 1 1 0-2.66h.06a1.11 1.11 0 0 0 .74-1.94l-.04-.04a1.33 1.33 0 1 1 1.88-1.88l.04.04a1.11 1.11 0 0 0 1.21.22H6a1.11 1.11 0 0 0 .67-1.02v-.11a1.33 1.33 0 1 1 2.66 0v.06a1.11 1.11 0 0 0 1.88.79l.04-.04a1.33 1.33 0 1 1 1.88 1.88l-.04.04a1.11 1.11 0 0 0-.22 1.21V6a1.11 1.11 0 0 0 1.02.67h.11a1.33 1.33 0 1 1 0 2.66h-.06a1.11 1.11 0 0 0-1.02.67z" />
  {:else if name === 'star'}
    <path d="M8 1.7l1.95 3.95 4.35.64-3.15 3.07.74 4.34L8 11.65l-3.89 2.05.74-4.34L1.7 6.29l4.35-.64z" />
  {:else if name === 'expand'}
    <path d="M9.6 1.9h4.5v4.5M6.4 14.1H1.9V9.6M14.1 1.9L8.9 7.1M1.9 14.1l5.2-5.2" />
  {:else if name === 'collapse'}
    <path d="M6.4 1.9v4.5H1.9M9.6 14.1V9.6h4.5M1.9 6.4l5.2-5.2M14.1 9.6l-5.2 5.2" />
  {:else if name === 'sun'}
    <circle cx="8" cy="8" r="3.1" />
    <path d="M8 1v1.6M8 13.4V15M15 8h-1.6M2.6 8H1M12.95 3.05l-1.13 1.13M4.18 11.82l-1.13 1.13M12.95 12.95l-1.13-1.13M4.18 4.18L3.05 3.05" />
  {:else if name === 'moon'}
    <path d="M13.5 9.6A5.9 5.9 0 0 1 6.4 2.5a5.9 5.9 0 1 0 7.1 7.1z" />
  {:else if name === 'monitor'}
    <rect x="1.8" y="2.8" width="12.4" height="8.4" rx="1.3" />
    <path d="M5.8 14h4.4M8 11.2V14" />
  {:else if name === 'chevron-down'}
    <path d="M4 6l4 4 4-4" />
  {:else if name === 'chevron-right'}
    <path d="M6 4l4 4-4 4" />
  {/if}
</svg>

<style>
  .icon {
    display: inline-block;
    vertical-align: -0.15em;
    flex: none;
  }
</style>
