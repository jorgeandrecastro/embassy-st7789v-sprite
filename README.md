# embassy-st7789v-sprite

Moteur de sprites et d'animation style **Piskel**, avec **framebuffer RAM**,
pour l'écran **ST7789V** (240×320) piloté via **Embassy**.

[![Crates.io](https://img.shields.io/crates/v/embassy-st7789v-sprite.svg)](https://crates.io/crates/embassy-st7789v-sprite)
[![Docs.rs](https://docs.rs/embassy-st7789v-sprite/badge.svg)](https://docs.rs/embassy-st7789v-sprite)
[![License: GPL-2.0-or-later](https://img.shields.io/badge/license-GPL--2.0--or--later-blue.svg)](LICENSE)
![no_std](https://img.shields.io/badge/no__std-yes-brightgreen)

---

## Sommaire

- [Principe](#principe)
- [Installation](#installation)
- [Démarrage rapide](#démarrage-rapide)
- [Concepts clés](#concepts-clés)
  - [Le framebuffer](#le-framebuffer)
  - [Sprites Piskel](#sprites-piskel)
  - [Transparence](#transparence)
  - [Clipping automatique](#clipping-automatique)
- [Référence API](#référence-api)
  - [Constantes](#constantes)
  - [`PiskelSprite`](#piskelsprite)
  - [`SpriteEngine`](#spriteengine)
- [Exemple d'intégration complet](#exemple-dintégration-complet)
- [Exporter un sprite depuis Piskel](#exporter-un-sprite-depuis-piskel)
- [Performances et contraintes mémoire](#performances-et-contraintes-mémoire)
- [Licence](#licence)

---

## Principe

Tout le dessin s'effectue dans un **buffer RAM de 153 600 octets**
(240 × 320 pixels, encodés en RGB565 sur 16 bits chacun).

Les opérations de dessin (`clear`, `draw_sprite`, `draw_pixel`, `fill_rect`)
sont **synchrones** et ne font que modifier ce buffer en mémoire — **aucune**
communication SPI n'a lieu à ce stade. Une fois la scène composée (fond,
sprites, HUD…), un seul appel asynchrone à `flush().await` envoie la frame
complète vers l'écran physique via SPI/DMA.

Ce découpage (composition en RAM puis envoi en un bloc) évite tout
scintillement et correspond à une double-bufferisation logicielle, adaptée
aux contraintes mémoire de l'embarqué.

```text
┌─────────────┐   clear() / draw_sprite() / fill_rect()   ┌─────────────┐
│   Votre      │ ─────────────────────────────────────── │ Framebuffer  │
│   logique    │                                           │ RAM (RAM)    │
│   de jeu     │ ←──────────── flush().await ───────────  │ 153 600 o.   │
└─────────────┘             (un seul transfert SPI)        └─────────────┘
                                                                   │
                                                                   ▼
                                                            Écran ST7789V
```

## Installation

Ajoutez la dépendance dans votre `Cargo.toml` :

```toml
[dependencies]
embassy-st7789v-sprite = "0.1"
embassy-st7789v        = "0.3"
embedded-hal            = "1.0"
embedded-hal-async      = "1.0"
```

Le crate est `#![no_std]` et `#![forbid(unsafe_code)]` : aucune dépendance
sur `alloc`, aucun `unsafe` dans la bibliothèque elle-même (le `unsafe`
éventuel pour une variable `static mut` reste à la charge de l'utilisateur,
voir l'exemple ci-dessous).

## Démarrage rapide

```rust,ignore
use embassy_st7789v_sprite::{SpriteEngine, PiskelSprite, FB_SIZE, TRANSPARENT_KEY};
use embassy_st7789v::Color;

// Sprite exporté depuis Piskel : 4 frames de 16x16 pixels, RGB565.
static HERO_PIXELS: [u16; 16 * 16 * 4] = [ /* ... données exportées ... */ TRANSPARENT_KEY; 16 * 16 * 4];

static HERO_SPRITE: PiskelSprite = PiskelSprite {
    width: 16,
    height: 16,
    frame_count: 4,
    pixels: &HERO_PIXELS,
};

// Le framebuffer (153 600 octets) est trop gros pour la pile : on le place en `static`.
static mut FRAMEBUFFER: [u16; FB_SIZE] = [0u16; FB_SIZE];

async fn dessiner_une_frame(display: &mut embassy_st7789v::St7789v<impl embedded_hal_async::spi::SpiDevice, impl embedded_hal::digital::OutputPin>) {
    let framebuffer = unsafe { &mut *core::ptr::addr_of_mut!(FRAMEBUFFER) };
    let mut engine = SpriteEngine::new(display, framebuffer);

    engine.clear(Color(0x0000));                  // 1. fond noir
    engine.draw_sprite(&HERO_SPRITE, 100, 200, 0); // 2. héros, frame 0, position (100, 200)
    engine.flush().await.unwrap();                // 3. envoi complet vers l'écran
}
```

## Concepts clés

### Le framebuffer

- Taille fixe : `FB_SIZE = SCREEN_W * SCREEN_H = 76 800` pixels, soit
  **153 600 octets** (2 octets par pixel en RGB565).
- Il doit être fourni par l'appelant sous la forme `&mut [u16; FB_SIZE]`,
  généralement une variable `static mut` (la pile d'une tâche Embassy est
  bien trop petite pour l'accueillir).
- `SpriteEngine` n'en prend jamais la propriété : il emprunte une référence
  mutable pendant sa durée de vie (`'a`).

### Sprites Piskel

Un [`PiskelSprite`](#piskelsprite) représente une planche de sprite exportée
depuis [Piskel](https://www.piskelapp.com/) :

- les pixels sont stockés **en Flash** (`&'static [u16]`), pas en RAM ;
- toutes les frames d'une animation sont concaténées dans un seul tableau
  (frame 0, puis frame 1, etc.) ;
- chaque pixel est encodé en **RGB565**.

### Transparence

La couleur `TRANSPARENT_KEY` (`0xF81F`, magenta pur) est traitée comme
transparente par `draw_sprite` : tout pixel du sprite valant cette couleur
n'est **pas** copié dans le framebuffer, ce qui laisse apparaître le
contenu déjà dessiné en dessous (fond, autre sprite…).

> ⚠️ Choisissez cette couleur comme fond transparent lors de l'export
> Piskel, et évitez de vous en servir pour un détail réel du sprite.

### Clipping automatique

`draw_sprite`, `draw_pixel` et `fill_rect` acceptent des coordonnées
signées (`i16`) et effectuent un clipping automatique contre les bords de
l'écran `[0, 240[ × [0, 320[`. Un sprite peut donc être partiellement (ou
totalement) hors écran sans provoquer de panique ni de débordement mémoire.

## Référence API

### Constantes

| Constante | Type | Valeur | Description |
|---|---|---|---|
| `SCREEN_W` | `usize` | `240` | Largeur de l'écran ST7789V en pixels. |
| `SCREEN_H` | `usize` | `320` | Hauteur de l'écran ST7789V en pixels. |
| `FB_SIZE` | `usize` | `76 800` | Nombre total de pixels du framebuffer (`SCREEN_W × SCREEN_H`). |
| `TRANSPARENT_KEY` | `u16` | `0xF81F` | Couleur-clé RGB565 (magenta) traitée comme transparente par `draw_sprite`. |

### `PiskelSprite`

Structure représentant un sprite statique ou une planche d'animation.

```rust,ignore
pub struct PiskelSprite {
    pub width: u16,             // largeur d'une frame, en pixels
    pub height: u16,            // hauteur d'une frame, en pixels
    pub frame_count: u16,       // nombre total de frames
    pub pixels: &'static [u16], // pixels RGB565 de toutes les frames, en Flash
}
```

Exemple de construction :

```rust,ignore
static WALK_PIXELS: [u16; 32 * 32 * 6] = [ /* export Piskel */ ];

static PLAYER_WALK: PiskelSprite = PiskelSprite {
    width: 32,
    height: 32,
    frame_count: 6,
    pixels: &WALK_PIXELS,
};
```

### `SpriteEngine`

```rust,ignore
pub struct SpriteEngine<'a, SPI, DC, RST = NoPin>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin;
```

Moteur de rendu par framebuffer pour le ST7789V. Générique sur le bus SPI,
la broche DC (data/command) et, optionnellement, la broche RST (reset).

#### Constructeurs

| Fonction | Signature | Description |
|---|---|---|
| `SpriteEngine::new` | `fn new(display: &'a mut St7789v<SPI, DC, RST>, framebuffer: &'a mut [u16; FB_SIZE]) -> Self` | Crée un moteur pour un `St7789v` **avec** broche RST matérielle. |
| `SpriteEngine::new_no_rst` | `fn new_no_rst(display: &'a mut St7789v<SPI, DC, NoPin>, framebuffer: &'a mut [u16; FB_SIZE]) -> Self` | Crée un moteur pour un `St7789v` construit **sans** broche RST matérielle. |

```rust,ignore
// Avec broche RST
let mut engine = SpriteEngine::new(&mut display, framebuffer);

// Sans broche RST (RST relié au reset global du MCU, par exemple)
let mut engine = SpriteEngine::new_no_rst(&mut display, framebuffer);
```

#### Dessin (synchrone, RAM uniquement)

| Fonction | Signature | Description |
|---|---|---|
| `clear` | `fn clear(&mut self, color: Color)` | Remplit tout le framebuffer avec `color`. |
| `draw_sprite` | `fn draw_sprite(&mut self, sprite: &PiskelSprite, start_x: i16, start_y: i16, frame: u16)` | Dessine la frame `frame` du sprite à la position `(start_x, start_y)`, avec transparence et clipping automatiques. |
| `draw_pixel` | `fn draw_pixel(&mut self, x: i16, y: i16, color: Color)` | Dessine un pixel isolé (ignoré silencieusement si hors écran). |
| `fill_rect` | `fn fill_rect(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, color: Color)` | Remplit un rectangle `[x0, x1] × [y0, y1]` (coins inclusifs), clippé aux bords de l'écran. |

```rust,ignore
engine.clear(Color(0x0000));                       // écran noir
engine.fill_rect(0, 280, 239, 319, Color(0x0664));   // bande "sol" en bas d'écran
engine.draw_pixel(120, 160, Color(0xFFFF));          // un pixel blanc au centre
engine.draw_sprite(&HERO_SPRITE, 100, 200, frame_idx); // héros, frame courante
```

#### Envoi vers l'écran (asynchrone, SPI)

| Fonction | Signature | Description |
|---|---|---|
| `flush` | `async fn flush(&mut self) -> Result<(), SPI::Error>` | Envoie le contenu complet du framebuffer vers l'écran physique. |

```rust,ignore
engine.flush().await?;
```

#### Récapitulatif d'une boucle d'animation typique

```rust,ignore
loop {
    engine.clear(Color(0x867D));                       // 1. fond
    engine.fill_rect(0, 280, 239, 319, Color(0x0664));   // 2. décor statique
    engine.draw_sprite(&HERO_SPRITE, x, 264, frame);     // 3. sprite animé

    engine.flush().await.unwrap();                       // 4. envoi à l'écran

    x += 2;                                               // 5. mise à jour du jeu
    frame = (frame + 1) % HERO_SPRITE.frame_count;
    Timer::after(Duration::from_millis(33)).await;        // 6. ~30 FPS
}
```

## Exemple d'intégration complet

Un exemple complet, prêt à adapter à votre carte (initialisation SPI,
écran, boucle d'animation à ~30 FPS), est disponible dans
[`examples/integration_complete.rs`](examples/integration_complete.rs).

Il illustre notamment :

- l'initialisation du bus SPI et des broches DC/CS/RST via `embassy-rp`
  (à remplacer par votre propre HAL : `embassy-stm32`, `embassy-nrf`, …) ;
- la déclaration d'un `PiskelSprite` animé (4 frames de marche) ;
- la construction du `SpriteEngine` à partir d'un framebuffer `static` ;
- une boucle de jeu : `clear` → `fill_rect` (décor) → `draw_sprite`
  (personnage) → `flush().await` → mise à jour de l'état → `Timer::after`.

Lancez-le (une fois adapté à votre cible) avec :

```bash
cargo run --example integration_complete --release
```

## Exporter un sprite depuis Piskel

1. Dessinez votre sprite ou votre animation dans [Piskel](https://www.piskelapp.com/).
2. Utilisez **`0xF81F`** (magenta pur, `TRANSPARENT_KEY`) comme couleur de
   fond transparente.
3. Exportez chaque frame en RGB565, puis concaténez-les dans un seul
   tableau `&'static [u16]` (frame 0 en premier, puis frame 1, etc.).
4. Construisez un `PiskelSprite` avec la largeur/hauteur d'une frame, le
   nombre de frames, et une référence vers ce tableau.

```rust,ignore
// 3 frames de 24x24 pixels
static COIN_PIXELS: [u16; 24 * 24 * 3] = [ /* frame 0, frame 1, frame 2 */ ];

static COIN_SPRITE: PiskelSprite = PiskelSprite {
    width: 24,
    height: 24,
    frame_count: 3,
    pixels: &COIN_PIXELS,
};
```

## Performances et contraintes mémoire

- Le framebuffer occupe **153 600 octets** de RAM — vérifiez que votre
  microcontrôleur dispose de suffisamment de mémoire (par exemple, un
  RP2040 avec 264 Ko de SRAM peut l'accueillir, mais cela laisse peu de
  marge pour le reste de l'application).
- `flush()` envoie les 76 800 pixels **un par un** via `display.draw_pixel`
  (pas de transfert DMA en bloc au niveau de ce crate) : le coût dépend
  donc directement de la fréquence SPI configurée et de l'implémentation
  sous-jacente de `embassy-st7789v`.
- Toutes les opérations de dessin sont en `O(pixels affectés)` et ne
  provoquent aucune allocation (`#![no_std]`, pas de dépendance `alloc`).

## Licence

Distribué sous licence **GPL-2.0-or-later**. Voir [LICENSE](LICENSE) pour le texte complet.