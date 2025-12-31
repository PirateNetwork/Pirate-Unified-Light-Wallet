# Pirate Wallet Design System

A world-class, dark-first design system built for premium desktop and mobile experiences.

## Overview

This design system provides a complete foundation for building the Pirate Wallet UI with:

- **Design Tokens**: Centralized color, spacing, and typography tokens
- **Premium Dark Theme**: Optimized for OLED displays with high contrast
- **Component Library**: Atomic design methodology (atoms, molecules, organisms)
- **Motion System**: 60fps micro-interactions and page transitions  
- **Desktop Polish**: Frameless windows, custom titlebars, hover states
- **Accessibility**: WCAG AA compliant with high-contrast mode

## Philosophy

### Dark-First

The entire system is designed for dark environments:
- Base background: `#0A0B0E` (near-black)
- Progressive elevation system
- High contrast text hierarchy
- Optimized semantic colors for dark UI

### No Script-Kiddie Look

This is not a Material Design clone or template-generated UI:
- Custom gradient system (not generic blues)
- Bespoke component designs
- Premium shadows and borders
- Optical sizing for typography
- Desktop-class interactions

### 60fps or Bust

All interactions are GPU-accelerated:
- Hardware-accelerated transforms
- Optimized repaints
- Smooth curves (custom easing)
- No jank, ever

## Structure

```
lib/
├── design/                 # Design system core
│   ├── tokens/             # Design tokens
│   │   ├── colors.dart     # Color palette & gradients
│   │   ├── spacing.dart    # Spacing scale & layout
│   │   └── typography.dart # Type scale & fonts
│   └── theme.dart          # Material Theme builder
│
├── ui/                     # Component library
│   ├── atoms/              # Basic building blocks
│   │   ├── p_button.dart
│   │   ├── p_input.dart
│   │   ├── p_checkbox.dart
│   │   ├── p_radio.dart
│   │   ├── p_toggle.dart
│   │   ├── p_badge.dart
│   │   └── p_tag.dart
│   │
│   ├── molecules/          # Compound components
│   │   ├── p_card.dart
│   │   ├── p_list_tile.dart
│   │   ├── p_form_section.dart
│   │   ├── p_snack.dart
│   │   ├── p_dialog.dart
│   │   └── p_bottom_sheet.dart
│   │
│   ├── organisms/          # Complex components
│   │   ├── p_scaffold.dart
│   │   ├── p_nav.dart
│   │   ├── p_hero_header.dart
│   │   └── p_skeleton.dart
│   │
│   └── motion/             # Animation system
│       ├── curves.dart
│       ├── durations.dart
│       ├── transitions.dart
│       └── micro_interactions.dart
│
└── features/
    └── showcase/           # Component showcase
        └── screens/
            ├── showcase_home_screen.dart
            ├── showcase_buttons_screen.dart
            ├── showcase_forms_screen.dart
            ├── showcase_cards_screen.dart
            ├── showcase_dialogs_screen.dart
            └── showcase_animations_screen.dart
```

## Design Tokens

### Colors (`PColors`)

#### Background Layers
- `backgroundBase` - #0A0B0E (deepest)
- `backgroundSurface` - #0F1116 (cards, panels)
- `backgroundElevated` - #141822 (modals, dialogs)

#### Accent Gradients
- **Gradient A**: #7CF4F7 → #6C8CFF (cyan to blue)
- **Gradient B**: #B072FF → #FF7EDB (purple to pink)

#### Text Hierarchy
- `textPrimary` - 95% opacity (headings)
- `textSecondary` - 70% opacity (body)
- `textTertiary` - 50% opacity (labels)
- `textDisabled` - 30% opacity (disabled)

#### Semantic Colors
Optimized for dark UI with subtle backgrounds:
- Success: #00E5B4 (green)
- Warning: #FFB800 (orange)
- Error: #FF5757 (red)
- Info: #7C9CFF (blue)

#### Interactive States
- Focus ring: #7CF4F7 (2px)
- Hover overlay: 5% white
- Pressed overlay: 10% white
- Selected background: 10% cyan

### Spacing (`PSpacing`)

4px/8px base scale:
- `xxs` - 4px
- `xs` - 8px
- `sm` - 12px
- `md` - 16px (base unit)
- `lg` - 24px
- `xl` - 32px
- `xxl` - 48px
- `xxxl` - 64px

Border radius:
- `radiusXS` - 4px
- `radiusSM` - 8px
- `radiusMD` - 12px (default)
- `radiusLG` - 16px
- `radiusXL` - 24px
- `radiusFull` - pill shape

### Typography (`PTypography`)

#### Font Families
- **Inter**: UI text (variable font with optical sizing)
- **JetBrains Mono**: Code, addresses, hashes

#### Type Scale
- Display: 48px - 72px (hero sections)
- Heading: 18px - 40px (section headers)
- Title: 16px - 22px (emphasized text)
- Body: 14px - 18px (content)
- Label: 12px - 16px (UI elements)
- Caption: 11px - 12px (metadata)
- Code: 12px - 16px (monospace)

#### Features
- Tabular figures for numbers
- Optical sizing for large text
- Proper line heights (1.2 - 1.7)
- Letter spacing adjustments

## Components

### Atoms

#### PButton
Primary action button with variants:
- `primary` - Gradient A (default)
- `secondary` - Gradient B
- `outline` - Transparent with border
- `ghost` - No background
- `danger` - Error color

Sizes: `small`, `medium`, `large`

Features:
- Hover states with elevation
- Press animation (scale down)
- Loading state
- Icon support
- Full-width option

#### PIconButton
Icon-only button with tooltip:
- Hover background
- Three sizes
- Disabled state

### Molecules

(To be implemented)

### Organisms

(To be implemented)

## Motion System

### Curves
- `easeInOut` - Default curve
- `easeOut` - Exit animations
- `spring` - Bouncy interactions
- `snap` - Instant feedback

### Durations
- Fast: 100-150ms (micro-interactions)
- Normal: 200-300ms (transitions)
- Slow: 400-500ms (page transitions)

### Micro-Interactions
- Button press: scale to 0.95
- Hover: fade overlay 150ms
- Focus: ring fade in 100ms
- Loading: spinner rotation

## Desktop Features

### Window Management
- Frameless window (no OS chrome)
- Custom titlebar with draggable area
- System buttons (minimize, maximize, close)
- Remember window bounds
- Minimum size: 960x600px

### Platform Features
- **Windows**: Mica/Acrylic backdrop
- **macOS**: Vibrancy effects
- **Linux**: Standard backdrop

### Mouse/Trackpad
- Hover states on all interactive elements
- Smooth scroll physics
- Pointer cursor on buttons
- Context menus
- Drag-and-drop support

## Accessibility

### WCAG AA Compliance
- 4.5:1 text contrast ratio
- 3:1 UI component contrast
- Focus indicators (2px ring)
- Keyboard navigation
- Screen reader support

### High Contrast Mode
Enable with `PTheme.dark(highContrast: true)`:
- Increased border opacity
- Brighter focus ring
- Enhanced text contrast

## Usage Examples

### Basic Button
```dart
PButton(
  onPressed: () => print('Pressed'),
  child: Text('Click Me'),
)
```

### Loading Button
```dart
PButton(
  loading: true,
  onPressed: null,
  child: Text('Loading...'),
)
```

### Button with Icon
```dart
PButton(
  onPressed: () {},
  icon: Icon(Icons.send),
  child: Text('Send'),
  variant: PButtonVariant.primary,
  size: PButtonSize.large,
)
```

### Full-Width Button
```dart
PButton(
  onPressed: () {},
  child: Text('Continue'),
  fullWidth: true,
)
```

### Using Tokens Directly
```dart
Container(
  padding: EdgeInsets.all(PSpacing.md),
  decoration: BoxDecoration(
    color: PColors.backgroundSurface,
    borderRadius: BorderRadius.circular(PSpacing.radiusMD),
    border: Border.all(
      color: PColors.borderDefault,
      width: 1.0,
    ),
  ),
  child: Text(
    'Custom Component',
    style: PTypography.bodyMedium(color: PColors.textPrimary),
  ),
)
```

## Golden Tests

All components have golden tests:

```bash
# Generate golden files
flutter test --update-goldens

# Run golden tests
flutter test test/ui/
```

## Performance

- 60fps on all interactions
- < 16ms frame time
- GPU-accelerated transforms
- Optimized rebuilds with const constructors
- Efficient theme lookups

## Browser DevTools

View design tokens in Chrome DevTools:
1. Open Flutter DevTools
2. Navigate to "Inspector"
3. Select any component
4. View applied tokens in properties panel

## Contributing

When adding new components:
1. Use existing tokens (don't add new colors)
2. Follow atomic design methodology
3. Add hover/press/focus states
4. Include desktop-specific interactions
5. Write golden tests
6. Update this documentation

## Resources

- [Atomic Design](https://atomicdesign.bradfrost.com/)
- [Material 3](https://m3.material.io/)
- [Flutter Animations](https://docs.flutter.dev/ui/animations)
- [WCAG Guidelines](https://www.w3.org/WAI/WCAG21/quickref/)

---

**Remember**: Every pixel matters. This is the user's first impression of Pirate Wallet. Make it count. 🏴‍☠️

