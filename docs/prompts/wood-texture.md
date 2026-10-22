# Prompt for ChatGPT: wood texture that works in Genix

Paste the prompt as is (English works best for image generation). Ask for one image at a time.

## Albedo (colour) map

```
Create a seamless tileable wood texture, perfectly square, 2048x2048 pixels.

View: strictly top-down, orthographic, camera perpendicular to the surface. Flat plank surface that fills the whole image edge to edge. No perspective, no depth of field, no vignette.

Material: natural oak wood, straight parallel grain running horizontally, fine pores, a few small subtle knots far from the edges. Warm medium-brown tone with gentle natural variation.

Lighting: completely flat, even, diffuse light. NO shadows, NO highlights, NO reflections, NO glossy sheen, NO gradient from one side to the other. The image must look like a scanned albedo (base colour) map, not a photo of a board.

Tileability: the left edge must continue exactly into the right edge and the top edge into the bottom edge, with no visible seam, no brightness jump and no grain that ends at the border. Do not put any knot, crack or distinct feature on the edges.

Forbidden: text, watermark, logo, frame, border, nails, screws, plank gaps, cut ends, background, floor, table, hands, props.

Output: just the texture image, nothing else.
```

## Optional companion maps (ask in the same chat, after the albedo is accepted)

```
Using exactly the same wood texture, produce a matching tileable grayscale roughness map (matte wood, mostly mid-grey, pores slightly darker/rougher), 2048x2048, same alignment as the colour map, no text.
```

```
Using exactly the same wood texture, produce a matching tileable tangent-space normal map (the usual blue-purple look), 2048x2048, subtle grain relief only, same alignment, no text.
```

## Check before using

- Put the image next to a copy of itself (2x2): no visible seams.
- Same pixel size for all maps, square, power of two.
- If a seam shows up, ask: "Make it tile seamlessly: offset the image by half its width and height, then repaint the cross-shaped seam area so it blends, and give me the result."
