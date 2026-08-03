#![no_std]
#![forbid(unsafe_code)]

//! # embassy-st7789v-sprite
//!
//! Moteur de sprites et d'animation style Piskel avec Framebuffer RAM
//! pour l'écran ST7789V 240×320 via Embassy.
//!
//! ## Principe
//!
//! Tout le dessin s'effectue dans un buffer RAM de 153 600 octets
//! (`SCREEN_W` × `SCREEN_H` pixels codés en RGB565, soit 2 octets/pixel).
//! Toutes les opérations de dessin ([`SpriteEngine::clear`], [`SpriteEngine::draw_sprite`],
//! [`SpriteEngine::draw_pixel`], [`SpriteEngine::fill_rect`], [`SpriteEngine::draw_str`],
//! [`SpriteEngine::draw_u32`], [`SpriteEngine::draw_f32`]) sont **synchrones** et
//! ne font que modifier le framebuffer en RAM, sans aucune communication SPI.
//! Un seul appel [`SpriteEngine::flush`] (asynchrone) envoie ensuite la frame
//! complète vers l'écran physique via SPI/DMA.
//!
//! Ce découpage permet de composer une scène complète (fond, sprites, HUD…)
//! sans scintillement, puis de l'envoyer d'un bloc une technique de
//! double-bufferisation logicielle adaptée aux contraintes mémoire de l'embarqué.
//!
//! ## Pourquoi une police locale plutôt que celle de `embassy-st7789v` ?
//!
//! `St7789v::draw_str` (dans `embassy-st7789v`) écrit **directement en SPI**,
//! en dehors de tout framebuffer. Ce crate maintient son propre buffer
//! `[u16; FB_SIZE]` composé en RAM puis envoyé d'un bloc via [`SpriteEngine::flush`] ;
//! le texte doit donc être écrit dans *ce* buffer pour rester synchronisé avec
//! les sprites, sinon on obtient un clignotement (texte et sprites envoyés par
//! deux chemins SPI distincts). D'où une copie locale de la police 5×7.

use embedded_hal::digital::OutputPin;
use embedded_hal_async::spi::SpiDevice;

// Réexport de tout ce qui est utile pour l'utilisateur final.
pub use embassy_st7789v::{
    Color, NoPin, St7789v, St7789vBuffered,
    APPROX, DEGREE, DELTA, DIVIDE, GE, INFINITY, LAMBDA, LE, PI, PLUS_MINUS, SQRT, THETA, TIMES,
};

/// Largeur de l'écran ST7789V en pixels.
pub const SCREEN_W: usize = 240;

/// Hauteur de l'écran ST7789V en pixels.
pub const SCREEN_H: usize = 320;

/// Nombre total de pixels du framebuffer (SCREEN_W × SCREEN_H).
pub const FB_SIZE: usize = SCREEN_W * SCREEN_H; // 76 800 pixels → 153 600 octets

/// Couleur-clé de transparence utilisée par [`SpriteEngine::draw_sprite`].
pub const TRANSPARENT_KEY: u16 = 0xF81F;

// ─────────────────────────────────────────────────────────────────────────────
// Police bitmap 5×7:copie locale, indépendante de embassy-st7789v.
// Mêmes glyphes/index que la police du pilote, pour rester cohérent visuellement.
// ─────────────────────────────────────────────────────────────────────────────

const FONT: [[u8; 5]; 73] = [
    [0x3E, 0x51, 0x49, 0x45, 0x3E], // 0
    [0x00, 0x42, 0x7F, 0x40, 0x00], // 1
    [0x42, 0x61, 0x51, 0x49, 0x46], // 2
    [0x21, 0x41, 0x45, 0x4B, 0x31], // 3
    [0x18, 0x14, 0x12, 0x7F, 0x10], // 4
    [0x27, 0x45, 0x45, 0x45, 0x39], // 5
    [0x3C, 0x4A, 0x49, 0x49, 0x30], // 6
    [0x01, 0x71, 0x09, 0x05, 0x03], // 7
    [0x36, 0x49, 0x49, 0x49, 0x36], // 8
    [0x06, 0x49, 0x49, 0x29, 0x1E], // 9
    [0x08, 0x08, 0x08, 0x08, 0x08], // 10 = '-'
    [0x00, 0x00, 0x00, 0x00, 0x00], // 11 = ' '
    [0x7E, 0x11, 0x11, 0x11, 0x7E], // 12 = 'A'
    [0x7F, 0x49, 0x49, 0x49, 0x36], // 13 = 'B'
    [0x3E, 0x41, 0x41, 0x41, 0x22], // 14 = 'C'
    [0x7F, 0x41, 0x41, 0x22, 0x1C], // 15 = 'D'
    [0x7F, 0x49, 0x49, 0x49, 0x41], // 16 = 'E'
    [0x7F, 0x09, 0x09, 0x09, 0x01], // 17 = 'F'
    [0x3E, 0x41, 0x49, 0x49, 0x7A], // 18 = 'G'
    [0x7F, 0x08, 0x08, 0x08, 0x7F], // 19 = 'H'
    [0x00, 0x41, 0x7F, 0x41, 0x00], // 20 = 'I'
    [0x20, 0x40, 0x41, 0x3F, 0x01], // 21 = 'J'
    [0x7F, 0x08, 0x14, 0x22, 0x41], // 22 = 'K'
    [0x7F, 0x40, 0x40, 0x40, 0x40], // 23 = 'L'
    [0x7F, 0x02, 0x0C, 0x02, 0x7F], // 24 = 'M'
    [0x7F, 0x04, 0x08, 0x10, 0x7F], // 25 = 'N'
    [0x3E, 0x41, 0x41, 0x41, 0x3E], // 26 = 'O'
    [0x7F, 0x09, 0x09, 0x09, 0x06], // 27 = 'P'
    [0x3E, 0x41, 0x51, 0x21, 0x5E], // 28 = 'Q'
    [0x7F, 0x09, 0x19, 0x29, 0x46], // 29 = 'R'
    [0x46, 0x49, 0x49, 0x49, 0x31], // 30 = 'S'
    [0x01, 0x01, 0x7F, 0x01, 0x01], // 31 = 'T'
    [0x3F, 0x40, 0x40, 0x40, 0x3F], // 32 = 'U'
    [0x1F, 0x20, 0x40, 0x20, 0x1F], // 33 = 'V'
    [0x3F, 0x40, 0x38, 0x40, 0x3F], // 34 = 'W'
    [0x63, 0x14, 0x08, 0x14, 0x63], // 35 = 'X'
    [0x07, 0x08, 0x70, 0x08, 0x07], // 36 = 'Y'
    [0x61, 0x51, 0x49, 0x45, 0x43], // 37 = 'Z'
    [0x00, 0x00, 0x60, 0x60, 0x00], // 38 = '.'
    [0x00, 0x3E, 0x41, 0x41, 0x00], // 39 = '('
    [0x00, 0x41, 0x41, 0x3E, 0x00], // 40 = ')'
    [0x00, 0x40, 0x50, 0x30, 0x00], // 41 = ','
    [0x00, 0x7F, 0x41, 0x41, 0x00], // 42 = '['
    [0x00, 0x41, 0x41, 0x7F, 0x00], // 43 = ']'
    [0x23, 0x13, 0x08, 0x64, 0x62], // 44 = '%'
    [0x08, 0x14, 0x22, 0x41, 0x00], // 45 = '<'
    [0x00, 0x41, 0x22, 0x14, 0x08], // 46 = '>'
    [0x00, 0x24, 0x24, 0x24, 0x00], // 47 = '='
    [0x02, 0x01, 0x51, 0x09, 0x06], // 48 = '?'
    [0x00, 0x00, 0x5F, 0x00, 0x00], // 49 = '!'
    [0x00, 0x36, 0x36, 0x00, 0x00], // 50 = ':'
    [0x08, 0x08, 0x3E, 0x08, 0x08], // 51 = '+'
    [0x20, 0x10, 0x08, 0x04, 0x02], // 52 = '/'
    [0x00, 0x00, 0x7F, 0x00, 0x00], // 53 = '|'
    [0x40, 0x40, 0x40, 0x40, 0x40], // 54 = '_'
    [0x04, 0x02, 0x01, 0x02, 0x04], // 55 = '^'
    [0x14, 0x7F, 0x14, 0x7F, 0x14], // 56 = '#'
    [0x3E, 0x41, 0x5D, 0x55, 0x1E], // 57 = '@'
    [0x32, 0x49, 0x55, 0x22, 0x50], // 58 = '&'
    [0x00, 0x07, 0x00, 0x07, 0x00], // 59 = '"'
    [0x60, 0x10, 0x0F, 0x10, 0x60], // 60 = 'λ'
    [0x3E, 0x49, 0x49, 0x49, 0x3E], // 61 = 'θ'
    [0x01, 0x7D, 0x01, 0x7D, 0x01], // 62 = 'π'
    [0x78, 0x46, 0x41, 0x46, 0x78], // 63 = 'Δ'
    [0x06, 0x09, 0x09, 0x06, 0x00], // 64 = '°'
    [0x48, 0x48, 0x5E, 0x48, 0x48], // 65 = '±'
    [0x22, 0x14, 0x08, 0x14, 0x22], // 66 = '×'
    [0x08, 0x08, 0x49, 0x08, 0x08], // 67 = '÷'
    [0x70, 0x4C, 0x03, 0x01, 0x01], // 68 = '√'
    [0x1C, 0x22, 0x1C, 0x22, 0x1C], // 69 = '∞'
    [0x00, 0x24, 0x12, 0x12, 0x24], // 70 = '≈'
    [0x60, 0x68, 0x64, 0x62, 0x61], // 71 = '≤'
    [0x61, 0x62, 0x64, 0x68, 0x60], // 72 = '≥'
];

fn char_to_glyph(c: u8) -> Option<usize> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as usize),
        b'-'        => Some(10),
        b' '        => Some(11),
        b'A'..=b'Z' => Some((c - b'A') as usize + 12),
        b'a'..=b'z' => Some((c - b'a') as usize + 12),
        b'.'        => Some(38),
        b'('        => Some(39),
        b')'        => Some(40),
        b','        => Some(41),
        b'['        => Some(42),
        b']'        => Some(43),
        b'%'        => Some(44),
        b'<'        => Some(45),
        b'>'        => Some(46),
        b'='        => Some(47),
        b'?'        => Some(48),
        b'!'        => Some(49),
        b':'        => Some(50),
        b'+'        => Some(51),
        b'/'        => Some(52),
        b'|'        => Some(53),
        b'_'        => Some(54),
        b'^'        => Some(55),
        b'#'        => Some(56),
        b'@'        => Some(57),
        b'&'        => Some(58),
        b'"'        => Some(59),
        LAMBDA      => Some(60),
        THETA       => Some(61),
        PI          => Some(62),
        DELTA       => Some(63),
        DEGREE      => Some(64),
        PLUS_MINUS  => Some(65),
        TIMES       => Some(66),
        DIVIDE      => Some(67),
        SQRT        => Some(68),
        INFINITY    => Some(69),
        APPROX      => Some(70),
        LE          => Some(71),
        GE          => Some(72),
        _           => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PiskelSprite
// ─────────────────────────────────────────────────────────────────────────────

/// Sprite statique ou planche d'animation exportée depuis Piskel.
///
/// Les données de pixels sont stockées en Flash (`&'static [u16]`).
/// Chaque pixel est encodé en **RGB565** sur 16 bits.
/// Le pixel [`TRANSPARENT_KEY`] est traité comme transparent par
/// [`SpriteEngine::draw_sprite`].
#[derive(Clone, Copy)]
pub struct PiskelSprite {
    /// Largeur d'une frame en pixels.
    pub width: u16,
    /// Hauteur d'une frame en pixels.
    pub height: u16,
    /// Nombre total de frames dans la planche.
    pub frame_count: u16,
    /// Données RGB565 de toutes les frames, en Flash.
    pub pixels: &'static [u16],
}

// ─────────────────────────────────────────────────────────────────────────────
// SpriteEngine
// ─────────────────────────────────────────────────────────────────────────────

/// Moteur de rendu par framebuffer pour le ST7789V.
pub struct SpriteEngine<'a, SPI, DC, RST = NoPin>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
{
    display: &'a mut St7789v<SPI, DC, RST>,
    framebuffer: &'a mut [u16; FB_SIZE],
}

// ── Constructeur sans broche RST ─────────────────────────────────────────────

impl<'a, SPI, DC> SpriteEngine<'a, SPI, DC, NoPin>
where
    SPI: SpiDevice,
    DC: OutputPin,
{
    /// Crée un moteur pour un `St7789v` construit **sans** broche RST matérielle.
    #[inline]
    pub fn new_no_rst(
        display: &'a mut St7789v<SPI, DC, NoPin>,
        framebuffer: &'a mut [u16; FB_SIZE],
    ) -> Self {
        Self { display, framebuffer }
    }
}

// ── Constructeur avec broche RST ─────────────────────────────────────────────

impl<'a, SPI, DC, RST> SpriteEngine<'a, SPI, DC, RST>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
{
    /// Crée un moteur pour un `St7789v` avec broche RST matérielle.
    #[inline]
    pub fn new(
        display: &'a mut St7789v<SPI, DC, RST>,
        framebuffer: &'a mut [u16; FB_SIZE],
    ) -> Self {
        Self { display, framebuffer }
    }

    // ── Opérations sur le framebuffer (synchrones, RAM uniquement) ────────────

    /// Remplit **tout** le framebuffer RAM avec `color` (aucune communication SPI).
    #[inline]
    pub fn clear(&mut self, color: Color) {
        self.framebuffer.fill(color.0);
    }

    /// Dessine la frame `frame` d'un [`PiskelSprite`] dans le framebuffer RAM.
    pub fn draw_sprite(
        &mut self,
        sprite: &PiskelSprite,
        start_x: i16,
        start_y: i16,
        frame: u16,
    ) {
        if frame >= sprite.frame_count {
            return;
        }

        let w = sprite.width as i16;
        let h = sprite.height as i16;
        let frame_stride = (sprite.width as usize) * (sprite.height as usize);
        let frame_offset = (frame as usize) * frame_stride;

        for py in 0..h {
            let screen_y = start_y + py;
            if screen_y < 0 || screen_y >= SCREEN_H as i16 {
                continue;
            }

            for px in 0..w {
                let screen_x = start_x + px;
                if screen_x < 0 || screen_x >= SCREEN_W as i16 {
                    continue;
                }

                let sprite_pixel_idx =
                    frame_offset + (py as usize) * (sprite.width as usize) + (px as usize);
                let raw_color = sprite.pixels[sprite_pixel_idx];

                if raw_color == TRANSPARENT_KEY {
                    continue;
                }

                let fb_idx = (screen_y as usize) * SCREEN_W + (screen_x as usize);
                self.framebuffer[fb_idx] = raw_color;
            }
        }
    }

    /// Dessine un pixel isolé directement dans le framebuffer RAM.
    #[inline]
    pub fn draw_pixel(&mut self, x: i16, y: i16, color: Color) {
        if x >= 0 && y >= 0 && (x as usize) < SCREEN_W && (y as usize) < SCREEN_H {
           self.framebuffer[(y as usize) * SCREEN_W + (x as usize)] = color.0;
        }
    }

    /// Remplit un rectangle dans le framebuffer RAM.
    pub fn fill_rect(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, color: Color) {
        let raw = color.0;
        let xa = x0.max(0) as usize;
        let ya = y0.max(0) as usize;
        let xb = (x1.min(SCREEN_W as i16 - 1)) as usize;
        let yb = (y1.min(SCREEN_H as i16 - 1)) as usize;

        if xa > xb || ya > yb {
            return;
        }

        for row in ya..=yb {
            let base = row * SCREEN_W;
            self.framebuffer[base + xa..=base + xb].fill(raw);
        }
    }

    // ── Texte (synchrone, RAM uniquement — même buffer que le reste) ──────────

    /// Dessine un glyphe 5×7 dans le framebuffer RAM (aucune communication SPI).
    /// Retourne la coordonnée x après le glyphe.
    pub fn draw_char(&mut self, x: i16, y: i16, glyph_idx: usize, fg: Color, bg: Color) -> i16 {
        for row in 0..7i16 {
            for col in 0..5i16 {
                let allume = (FONT[glyph_idx][col as usize] >> row) & 1 == 1;
                self.draw_pixel(x + col, y + row, if allume { fg } else { bg });
            }
        }
        x + 6
    }

    /// Affiche une chaîne ASCII (+ symboles étendus 0x80-0x8C) dans le framebuffer RAM.
    /// Retourne la coordonnée x après le dernier caractère.
    pub fn draw_str(&mut self, mut x: i16, y: i16, text: &[u8], fg: Color, bg: Color) -> i16 {
        for &c in text {
            if x + 5 >= SCREEN_W as i16 { break; }
            if let Some(idx) = char_to_glyph(c) {
                x = self.draw_char(x, y, idx, fg, bg);
            } else {
                x = x.saturating_add(6);
            }
        }
        x
    }

    /// Affiche un entier non signé 32 bits dans le framebuffer RAM.
    pub fn draw_u32(&mut self, mut x: i16, y: i16, val: u32, fg: Color, bg: Color) -> i16 {
        let mut n = val;
        let mut chiffres = [0u8; 10];
        let mut compte = 0usize;
        loop {
            chiffres[compte] = (n % 10) as u8;
            n /= 10;
            compte += 1;
            if n == 0 { break; }
        }
        for i in (0..compte).rev() {
            x = self.draw_char(x, y, chiffres[i] as usize, fg, bg);
        }
        x
    }

    /// Affiche un `f32` dans le framebuffer RAM, `decimales` chiffres après la virgule.
    /// Gère NaN et +/-Inf.
    pub fn draw_f32(&mut self, mut x: i16, y: i16, val: f32, decimales: u8, fg: Color, bg: Color) -> i16 {
        if val.is_nan() { return self.draw_str(x, y, b"NaN", fg, bg); }
        if val.is_infinite() {
            return self.draw_str(x, y, if val > 0.0 { b"+Inf" } else { b"-Inf" }, fg, bg);
        }
        let negatif = val < 0.0;
        let mut abs = if negatif { -val } else { val };
        if negatif { x = self.draw_char(x, y, 10, fg, bg); } // '-'

        let facteur = { let mut f = 1u32; for _ in 0..decimales { f *= 10; } f };
        abs += 0.5 / facteur as f32;

        let entier = abs as u32;
        x = self.draw_u32(x, y, entier, fg, bg);

        if decimales > 0 {
            x = self.draw_char(x, y, 38, fg, bg); // '.'
            let mut frac = abs - entier as f32;
            let mut chiffres = [0u8; 8];
            for i in 0..decimales as usize {
                frac *= 10.0;
                let d = frac as u8;
                chiffres[i] = d;
                frac -= d as f32;
            }
            for i in 0..decimales as usize {
                x = self.draw_char(x, y, chiffres[i] as usize, fg, bg);
            }
        }
        x
    }

    // ── Envoi vers l'écran (asynchrone, SPI) ─────────────────────────────────

    /// Envoie le contenu complet du framebuffer RAM vers l'écran physique
    /// en un seul flux SPI continu (fenêtre ouverte une fois, pas par pixel).
    pub async fn flush(&mut self) -> Result<(), SPI::Error> {
        self.display
            .blit_u16(0, 0, (SCREEN_W - 1) as u16, (SCREEN_H - 1) as u16, self.framebuffer)
            .await
    }
}