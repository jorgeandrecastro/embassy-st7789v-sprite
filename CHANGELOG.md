# Changelog

## 0.4.0

### Ajouté
- Nouvelles méthodes sur `SpriteEngine` : `draw_char`, `draw_str`,
  `draw_u32`, `draw_f32`: écrivent directement dans le framebuffer RAM
  du moteur, au même titre que `draw_sprite`/`draw_pixel`/`fill_rect`.
- Police bitmap 5×7 (73 glyphes, ASCII + symboles grecs/mathématiques)
  dupliquée localement dans ce crate, indépendante de celle de
  `embassy-st7789v` : aucune modification requise côté `embassy-st7789v`.

### Corrigé
- Clignotement du texte : auparavant, le texte devait être dessiné via
  `St7789v::draw_str`/`draw_u32`/`draw_f32` (écriture SPI directe, hors
  framebuffer), ce qui provoquait une désynchronisation visible entre le
  texte et le reste de la scène à chaque frame. Le texte suit désormais
  le même chemin que les sprites : composé en RAM, envoyé en un seul
  `flush()`.

### Modifié
- `embassy-st7789v` reste en `"0.6"` (aucun changement requis dans cette
  crate pour cette version  l'ajout de texte est entièrement local à
  `embassy-st7789v-sprite`).

## 0.3.0

### Modifié
- Mise à jour de la dépendance `embassy-st7789v` vers `0.6`.
- `SpriteEngine::flush()` utilise désormais `St7789v::blit_u16` pour
  envoyer le framebuffer entier en un seul transfert SPI continu
  (fenêtre ouverte une fois), ce qui permet des transferts plus
  efficaces via DMA si le pilote le supporte.

### Ajouté
- Documentation dans `README.md` expliquant le nouveau comportement
  de `flush()` et exemple d'implémentation.

## 0.2.0

### Ajouté
- Réexport des symboles grecs et mathématiques de `embassy-st7789v`
  (`LAMBDA`, `THETA`, `PI`, `DELTA`, `DEGREE`, `PLUS_MINUS`, `TIMES`,
  `DIVIDE`, `SQRT`, `INFINITY`, `APPROX`, `LE`, `GE`) pour l'affichage
  de texte scientifique via `draw_str` / `draw_str_buf`.

## 0.1.0
- Version initiale : `SpriteEngine`, `PiskelSprite`, framebuffer RAM,
  transparence, clipping automatique.