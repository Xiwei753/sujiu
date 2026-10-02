/*
 * UI-layer theme tokens.
 *
 * These tokens exist to name a *role* ("the page background", "text on the
 * accent fill"), not to choose a colour. Every value is a HarmonyOS system
 * colour resource, so the platform's own light and dark values, contrast
 * targets and future palette changes are what the app renders with.
 *
 * The earlier version of this file was a second palette: hand-picked hex
 * values with an `apply(dark)` branch, an Android-Material blue accent
 * (`#3D5AFE`) painted over save buttons and selected chips, and a blue
 * user bubble. That is a component library wearing the app's name, and it is
 * why ordinary controls looked like they belonged to some other product.
 *
 * Two consequences worth stating, because they look like lost features:
 *
 * 1. There is no light/dark branch any more. `sys.color.*` resolves against
 *    the application's colour mode, which `apply()` sets from the user's
 *    appearance choice. Light and Dark are therefore literally the same code
 *    path — they cannot drift, because there is nothing to drift.
 *
 * 2. There is no success green and no error fill. The platform's status
 *    palette is emphasis (selection/primary), alert (orange) and warning
 *    (red), and inventing a fourth semantic is how this file ended up
 *    disagreeing with the platform in the first place. Whether a probe
 *    succeeded is already stated in words by the state text itself, so it
 *    does not need a colour to be legible; only a failure is coloured.
 *
 * A token should be added here only for a role the platform genuinely cannot
 * express. "I wanted it blue" is not a role.
 */

import { common, ConfigurationConstant } from '@kit.AbilityKit';

// HarmonyOS system colours. Names verified against the SDK resource index.
// One constant per role rather than one object literal: ArkTS requires an
// object literal to correspond to a declared interface, and a `const` assertion
// is not accepted either, so a lookup table here would be three declarations of
// a type just to hold twelve resources that are each named once.

/** Page background behind the conversation canvas. */
const BACKGROUND: Resource = $r('sys.color.ohos_id_color_background');
/** Raised surface: composer input, cards, sheets. */
const CARD: Resource = $r('sys.color.ohos_id_color_card_bg');
/** Softer fill for chips, headers and inline blocks. */
const SUB_BACKGROUND: Resource = $r('sys.color.ohos_id_color_sub_background');
/** Hairline used for dividers and outlines. */
const SEPARATOR: Resource = $r('sys.color.ohos_id_color_list_separator');
/** Main readable text. */
const TEXT_PRIMARY: Resource = $r('sys.color.ohos_id_color_text_primary');
/** Secondary text: labels, metadata, timestamps. */
const TEXT_SECONDARY: Resource = $r('sys.color.ohos_id_color_text_secondary');
/** Selection and the single primary action. */
const EMPHASIZE: Resource = $r('sys.color.ohos_id_color_emphasize');
/**
 * Text on top of an emphasis fill.
 *
 * `font_on_primary`, not `ohos_id_color_emphasize_contrary`. The name reads
 * like the right one and is not: `emphasize_contrary` resolves to `#006cde`, the
 * emphasis blue itself, which is what a label on a *contrary* background wants
 * and is unreadable on an emphasis one. `font_on_primary` is `#FFFFFF` in both
 * modes, and it is the colour the platform's own emphasized button paints its
 * label with.
 */
const TEXT_ON_EMPHASIS: Resource = $r('sys.color.font_on_primary');
/** A tint rather than a surface, in either mode. */
const TINT: Resource = $r('sys.color.ohos_id_color_component_normal');
/** Error and destructive status. */
const WARNING: Resource = $r('sys.color.ohos_id_color_warning');
/**
 * The dim over content a drawer or sheet is covering.
 *
 * A mask rather than a flat colour, because it has to work over both the page
 * background and whatever is inside the covered area.
 */
const SCRIM: Resource = $r('sys.color.ohos_id_color_mask_regular');

@Observed
export class AppTheme {
  /** True when the resolved appearance is dark; kept for diagnostics. */
  isDark: boolean = false;

  /** Page background behind the conversation canvas. */
  readonly pageBackground: Resource = BACKGROUND;
  /** Raised surface: composer input, cards, sheets. */
  readonly surface: Resource = CARD;
  /** Softer fill for chips, headers and inline blocks. */
  readonly surfaceMuted: Resource = SUB_BACKGROUND;
  /** Hairline used for dividers and outlines. */
  readonly outline: Resource = SEPARATOR;
  /** Main readable text. */
  readonly textPrimary: Resource = TEXT_PRIMARY;
  /** Secondary text: labels, metadata, timestamps. */
  readonly textSecondary: Resource = TEXT_SECONDARY;
  /** Platform emphasis: selection, and the one primary action per screen. */
  readonly accent: Resource = EMPHASIZE;
  /** Text on top of an emphasis fill. */
  readonly textOnAccent: Resource = TEXT_ON_EMPHASIS;
  /** Tint for the user bubble, so it is not the loudest thing on screen. */
  readonly userBubble: Resource = TINT;
  /** Error and destructive status. */
  readonly danger: Resource = WARNING;
  /** The dim over covered content. */
  readonly scrim: Resource = SCRIM;

  constructor(dark: boolean) {
    this.isDark = dark;
  }

  /**
   * Records the resolved appearance and tells the platform what it is.
   *
   * The colours need no work — the platform resolves `sys.color.*` from the
   * application's colour mode. What does need doing is *setting* that mode, so
   * choosing Light or Dark actually forces it rather than only relabelling it.
   * Without this the tokens would keep rendering the system appearance no
   * matter which one the user picked here.
   */
  apply(dark: boolean, context?: common.Context): void {
    this.isDark = dark;
    if (context === undefined) {
      return;
    }
    const app = context.getApplicationContext();
    if (app === null) {
      return;
    }
    app.setColorMode(
      dark
        ? ConfigurationConstant.ColorMode.COLOR_MODE_DARK
        : ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT
    );
  }
}