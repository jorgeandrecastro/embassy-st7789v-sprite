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
//! [`SpriteEngine::draw_pixel`], [`SpriteEngine::fill_rect`]) sont **synchrones** et
//! ne font que modifier le framebuffer en RAM, sans aucune communication SPI.
//! Un seul appel [`SpriteEngine::flush`] (asynchrone) envoie ensuite la frame
//! complète vers l'écran physique via SPI/DMA.
//!
//! Ce découpage permet de composer une scène complète (fond, sprites, HUD…)
//! sans scintillement, puis de l'envoyer d'un bloc — une technique de
//! double-bufferisation logicielle adaptée aux contraintes mémoire de l'embarqué.
//!
//! ## Exemple minimal
//!
//! ```rust,ignore
//! use embassy_st7789v_sprite::{SpriteEngine, PiskelSprite, FB_SIZE};
//! use embassy_st7789v::Color;
//!
//! static HERO_SPRITE: PiskelSprite = PiskelSprite {
//!     width: 32,
//!     height: 32,
//!     frame_count: 4,
//!     pixels: &HERO_PIXELS, // généré depuis un export Piskel (voir README)
//! };
//!
//! // Le framebuffer doit vivre aussi longtemps que le moteur.
//! static mut FRAMEBUFFER: [u16; FB_SIZE] = [0u16; FB_SIZE];
//!
//! # async fn demo(display: &mut embassy_st7789v::St7789v<impl embedded_hal_async::spi::SpiDevice, impl embedded_hal::digital::OutputPin>) {
//! let framebuffer = unsafe { &mut *core::ptr::addr_of_mut!(FRAMEBUFFER) };
//! let mut engine = SpriteEngine::new(display, framebuffer);
//!
//! engine.clear(Color(0x0000));                    // fond noir
//! engine.draw_sprite(&HERO_SPRITE, 100, 200, 0);   // frame 0 du héros
//! engine.flush().await.unwrap();                   // envoi à l'écran
//! # }
//! # static HERO_PIXELS: [u16; 32 * 32 * 4] = [0u16; 32 * 32 * 4];
//! ```

use embassy_st7789v::{Color, NoPin, St7789v};
use embedded_hal::digital::OutputPin;
use embedded_hal_async::spi::SpiDevice;

/// Largeur de l'écran ST7789V en pixels.
pub const SCREEN_W: usize = 240;

/// Hauteur de l'écran ST7789V en pixels.
pub const SCREEN_H: usize = 320;

/// Nombre total de pixels du framebuffer (SCREEN_W × SCREEN_H).
pub const FB_SIZE: usize = SCREEN_W * SCREEN_H; // 76 800 pixels → 153 600 octets

/// Couleur-clé de transparence utilisée par [`SpriteEngine::draw_sprite`].
///
/// Tout pixel d'un [`PiskelSprite`] codé avec cette valeur RGB565
/// (magenta pur, `0xF81F`) n'est **pas** copié dans le framebuffer :
/// le fond existant (ciel, sol, autre sprite déjà dessiné…) reste visible.
///
/// Choisissez cette couleur comme fond transparent lors de l'export Piskel
/// (ou en dessinant vos sprites), en évitant de vous en servir pour un détail
/// réel du sprite.
pub const TRANSPARENT_KEY: u16 = 0xF81F;

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
    ///
    /// # Transparence
    ///
    /// Tout pixel du sprite valant [`TRANSPARENT_KEY`] (magenta `0xF81F`)
    /// est ignoré : le contenu déjà présent dans le framebuffer à cet
    /// emplacement (fond, autre sprite…) reste inchangé.
    ///
    /// # Clipping automatique
    ///
    /// Les coordonnées `start_x` / `start_y` sont signées (`i16`).
    /// Les pixels hors de la zone `[0, 240[ × [0, 320[` sont ignorés sans panique
    /// ni débordement : le sprite peut donc sortir partiellement du bord.
    ///
    /// # Panics
    ///
    /// Aucun panic. Un index de frame invalide (`frame >= frame_count`) est
    /// silencieusement ignoré.
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

                // Pixel transparent : on n'écrit pas dans le framebuffer,
                // ce qui laisse voir le fond déjà dessiné.
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
    ///
    /// `x0/y0` coin supérieur gauche, `x1/y1` coin inférieur droit (inclusifs).
    /// Clippé automatiquement contre les bords de l'écran.
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

    // ── Envoi vers l'écran (asynchrone, SPI) ─────────────────────────────────

    /// Envoie le contenu complet du framebuffer RAM vers l'écran physique.
    pub async fn flush(&mut self) -> Result<(), SPI::Error> {
        for y in 0..SCREEN_H {
            let row_start = y * SCREEN_W;
            for x in 0..SCREEN_W {
                let raw_color = self.framebuffer[row_start + x];
                let color = Color(raw_color);
                self.display.draw_pixel(x as u16, y as u16, color).await?;
            }
        }
        Ok(())
    }
}