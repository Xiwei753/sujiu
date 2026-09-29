/*
 * UI-layer theme tokens.
 *
 * The UI owns the palette. Presentation and platform services only supply the
 * fact "the system currently prefers dark"; they never pick colors. Every
 * component reads semantic tokens from here instead of hardcoding hex values,
 * so Light and Dark are the same code path.
 */

@Observed
export class AppTheme {
  isDark: boolean = false;

  /** Page background behind the conversation canvas. */
  pageBackground: string = '';
  /** Raised surface: composer input, cards, sheets. */
  surface: string = '';
  /** Softer fill for chips, headers and inline blocks. */
  surfaceMuted: string = '';
  /** Hairline used for dividers and outlines. */
  outline: string = '';
  /** Main readable text. */
  textPrimary: string = '';
  /** Secondary text: labels, metadata, timestamps. */
  textSecondary: string = '';
  /** Text on top of an accent fill. */
  textOnAccent: string = '';
  /** Single accent for selection and primary action. */
  accent: string = '';
  /** Accent used for the user bubble so it is not the loudest thing on screen. */
  userBubble: string = '';
  /** Error and destructive status. */
  danger: string = '';
  /** Translucent danger fill for inline error blocks. */
  dangerSurface: string = '';

  constructor(dark: boolean) {
    this.apply(dark);
  }

  /** Re-points every token in place so observed components re-render. */
  apply(dark: boolean): void {
    this.isDark = dark;
    if (dark) {
      this.pageBackground = '#0C0C10';
      this.surface = '#16161C';
      this.surfaceMuted = '#1F1F27';
      this.outline = '#2E2E38';
      this.textPrimary = '#E8E8F0';
      this.textSecondary = '#9A9AAB';
      this.textOnAccent = '#FFFFFF';
      this.accent = '#6E8BFF';
      this.userBubble = '#2A3A6B';
      this.danger = '#FF6B6B';
      this.dangerSurface = '#33B3261E';
    } else {
      this.pageBackground = '#F7F7FA';
      this.surface = '#FFFFFF';
      this.surfaceMuted = '#EFEFF4';
      this.outline = '#DCDCE4';
      this.textPrimary = '#17171C';
      this.textSecondary = '#5F5F6E';
      this.textOnAccent = '#FFFFFF';
      this.accent = '#3D5AFE';
      this.userBubble = '#DCE4FF';
      this.danger = '#B3261E';
      this.dangerSurface = '#14B3261E';
    }
  }
}
