// Lucide Icons - Icon component for RSPM Dashboard
// Using SVG sprite (https://lucide.dev/icons)

function Icon(name, className = 'icon', strokeWidth = 2) {
    return `<svg class="${className}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round"><use href="#icon-${name}"/></svg>`;
}