---
name: Genix
description: Flat rounded parametric CAD workspace inspired by iOS 18
colors:
  blue-light: "rgb(0, 101, 212)"
  blue-dark: "rgb(92, 166, 255)"
  indigo-light: "rgb(88, 70, 203)"
  indigo-dark: "rgb(164, 149, 255)"
  teal-light: "rgb(0, 117, 119)"
  teal-dark: "rgb(92, 212, 205)"
  orange-light: "rgb(168, 73, 0)"
  orange-dark: "rgb(255, 173, 92)"
  rose-light: "rgb(188, 42, 83)"
  rose-dark: "rgb(255, 135, 165)"
  canvas-light: "rgb(242, 242, 247)"
  surface-light: "rgb(255, 255, 255)"
  inset-light: "rgb(235, 235, 240)"
  border-light: "rgb(220, 220, 225)"
  text-light: "rgb(28, 28, 30)"
  secondary-light: "rgb(99, 99, 109)"
  canvas-dark: "rgb(0, 0, 0)"
  surface-dark: "rgb(28, 28, 30)"
  inset-dark: "rgb(44, 44, 46)"
  border-dark: "rgb(64, 64, 68)"
  text-dark: "rgb(245, 245, 247)"
  secondary-dark: "rgb(174, 174, 184)"
typography:
  body:
    fontSize: "14px"
  button:
    fontSize: "14px"
  heading:
    fontSize: "24px"
  library-title:
    fontSize: "26px"
rounded:
  control: "12px"
  primary-navigation: "14px"
  menu: "16px"
  model-card: "20px"
  surface: "22px"
  auth-dock: "24px"
spacing:
  item-horizontal: "8px"
  item-vertical: "7px"
  button-horizontal: "12px"
  button-vertical: "7px"
  window-margin: "20px"
components:
  button-primary:
    rounded: "{rounded.primary-navigation}"
    height: "42px"
  navigation:
    rounded: "{rounded.primary-navigation}"
    height: "42px"
  model-card:
    rounded: "{rounded.model-card}"
    width: "240px"
    height: "160px"
---

# Design System: Genix

## Overview

Genix uses flat rounded egui surfaces inspired by iOS 18, as explicitly requested by the user. Soft neutral containers, the existing Genix SVG logo and one configurable accent connect account pages, model cards and the CAD editor. This is an egui implementation, not native iOS components.

**Key Characteristics:**
- Flat neutral surfaces with rounded controls and containers.
- One configurable accent propagated through navigation, tools and feedback.
- Real geometry previews and immediate keyboard actions.

## Colors

### Primary
Blue is the default; Indigo, Teal, Orange and Rose are alternative accents with distinct light/dark values. Only one is configured at a time. Use theme::accent(), tint() and on_accent() for selection, links, tools, navigation, loading and non-error toasts. foreground_on() chooses black or white from luminance; dark-theme accents must not inherit an unconditional white label.

### Neutral
Light mode uses a cool pale canvas, white surfaces and slightly deeper input wells. Dark mode uses a black canvas, charcoal surfaces and gray wells. Use secondary() for supporting labels. Theme borders are restrained; most interactive controls are borderless at rest. CAD axes, construction geometry and error colors retain functional roles distinct from the configurable accent. The geometry viewport has its own flat neutral background values in theme.rs.

## Typography

On macOS, install_fonts attempts to read system SF Pro from SFNS.ttf at runtime and place it first in the proportional family. It is not bundled. If unavailable, egui defaults remain; bundled DejaVuSans and Hack supply additional language and symbol coverage. Body and button roles are 14 logical points and headings 24. The library title is 26; the gate and sidebar brand labels retain their local 32 and 23-point sizes. Preserve localization and fallback glyphs.

## Layout

All source dimensions are egui logical points; frontmatter px values are portable representations. The editor retains menus, tabs, toolbar, bottom timeline, resizable side panels and a geometry viewport. The rounded floating dock groups viewport controls without changing modeling flow.

Home navigation uses a 190-point sidebar at widths of 700 points or more and wraps horizontally below that threshold. Model cards wrap at fixed size. Profile, Updates and Server use a scrollable rounded surface with 28-point inner margin and maximum content width 640. The auth sheet uses 24-point padding and fits within a requested width of 392. Default control interaction height is 30; shared primary and navigation controls are 42. Preserve scrolling and wrapping on narrow windows.

## Elevation & Depth

Flat fills provide the main grouping; avoid adding decorative gradients or depth. Floating windows and popups retain a subtle shadow with offset (0, 8), blur 24, no spread and black alpha 18 in light mode or 64 in dark mode. Most surfaces and controls have no resting outline. Focus and selection use accent outlines where implemented.

## Shapes

Use the documented radius roles: general controls, primary/navigation controls, menus, cards, surfaces and auth/dock containers have distinct established sizes. Slider rails are filled, 6 points high, with circular handles and a default width of 140. Switches use a 42×26 track and a 10-point-radius thumb. Accent swatches use circular marks with a selected ring and contrast-aware checkmark.

## Components

- **Primary actions:** accent fill, computed foreground, strong label and 42-point minimum height. Pointer press overlays a subtle darkening; actions fire immediately.
- **Navigation:** 20-point SVG icons and 14-point labels in 42-point rows. Selection uses accent text and a translucent accent fill; keyboard focus has a 1.5-point outline. Pointer hover tint is suppressed for touch input.
- **Inputs and sliders:** native egui input behavior and rounded theme wells. Sliders have filled rails and circular handles. Expression fields preserve validation-specific colors and tooltips.
- **Switches and swatches:** shared custom switch supports pointer and keyboard activation; focus outlines the track. Swatches present the configured accent choices with check/ring selection feedback.
- **Model cards:** 240×160, 20-point corners, a 94-point-high inset preview area with 14-point corners, real aspect-fitted geometry, truncated title and smaller metadata. Hover/focus draws an accent outline; press blends the surface. Loading/unavailable previews retain explicit text.
- **Loading:** accent-colored 18-point spinner plus secondary text; reduced motion replaces rotation with a static dot.
- **Dock and toast:** viewport controls sit on a rounded flat container. Non-error toasts use the configured accent with computed foreground; errors retain semantic error color. Toasts appear for four seconds, enter over 200ms and fade during the final 400ms. Reduced motion skips entry but the timed exit fade remains in current source.

Shared pointer feedback uses 160ms cubic-bezier(0.23, 1, 0.32, 1), retargeting from the currently painted value on reversal. Keyboard events and the app reduced-motion preference resolve shared feedback immediately. Pointer page navigation raises opacity from 0.65 to 1 over 160ms; initial/programmatic pages are visible by default. The app preference disables page fades and spinner motion; it does not automatically read OS accessibility settings. Keep effects subordinate to CAD operations.

## Do's and Don'ts

### Do:
- **Do** use theme helpers for the configured accent and contrasting foreground.
- **Do** preserve real geometry previews, the Genix logo and existing CAD workflows.
- **Do** provide immediate keyboard actions and honor the app reduced-motion preference.
- **Do** keep local mock account and subscription status visible.

### Don't:
- **Don't** hard-code the old blue accent into new UI.
- **Don't** claim native iOS controls or automatic OS reduced-motion detection.
- **Don't** invent subscription pricing, storage promises or update availability.

## Search and check feedback

The library uses an interruptible 160 ms filter accordion with clipped height and opacity, reusing the shared ease-out. Keyboard activation and reduced motion resolve immediately. Shared checkbox controls keep a fixed hit area, animate the check path and accent fill, and compress the painted box on press. Accent swatches use the same check path; switches compress their thumb on press. Filtering updates immediately without moving result cards for decoration.

## Reference-driven backgrounds and spacing

User references: React Bits Pattern Waves (glyphs Genix, black background) for the model library; Shape Waves (circles, cell 12, brightness .43, direction 130°, flow 1.6, negative-space Genix) for account entry. Native egui adaptations in backgrounds.rs preserve the visual vocabulary with bounded geometry and 30 Hz repaint requests; this is not the original WebGL/WebGPU shader. Static with reduced motion; no background-driven repaints while unfocused. Light mode adapts contrast.

ReUI input/filters and Magic UI supplied secondary composition references. Search has a separate row, up to 680 logical pixels, 16px type and 16×13px inner padding. Auth fields use 16px type and 14×12px padding. Checkbox/radio check paths draw linearly over 320 ms per explicit user feedback; press feedback stays 160 ms. No logo icon on registration. Third-party notice is bundled and available under About.

## Background pointer response

Both native backgrounds passively track the mouse inside their clipped area. A 130px falloff highlights marks with the configured accent; movement and presses seed travelling rings at 180px/s, fading after 1.2s. At most 16 rings are retained and painted displacement is capped at 7px; content layout and hit targets never move. Touch does not generate hover response. Reduced motion keeps static local light and removes displacement, growth and rings.

## Library and menu corrections

Search includes an explicit vector magnifier with reserved left padding. Library section labels use 16px, result counts 14px. Light-mode pattern marks have independent contrast instead of the previous dark-theme opacity reduction. Menu hover/open highlights use 6px corners, while larger controls retain their established radii.
