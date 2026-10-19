/** 视觉规范：宣纸白 + 墨黑 + 朱砂。 */
export class Theme {
  static readonly PAPER: string = '#FAF8F4';
  static readonly CARD: string = '#FFFFFF';
  static readonly INK: string = '#1A1A1A';
  static readonly MUTED: string = '#6B6560';
  static readonly ACCENT: string = '#C8412F';
  static readonly ACCENT_LIGHT: string = '#F6E4E0';
  static readonly LINE: string = '#E8E2D8';

  static readonly RADIUS_CARD: number = 16;
  static readonly RADIUS_BUTTON: number = 20;
  static readonly RADIUS_THUMB: number = 12;

  static readonly PAGE_PADDING: number = 16;
  static readonly SHUTTER_SIZE: number = 72;

  static readonly EASE: Curve = Curve.FastOutSlowIn;
  static readonly DURATION: number = 240;

  static readonly SHADOW: ShadowOptions = {
    radius: 24,
    color: 'rgba(26,26,26,0.06)',
    offsetX: 0,
    offsetY: 8
  };
}
