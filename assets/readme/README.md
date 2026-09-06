# README illustrations

Supplemental illustrations for the existing README; the original hero and wordmark remain unchanged.

These are generated PNGs with a flat vector aesthetic, not editable SVG sources. Created with the built-in image generation tool. The primary style reference is [the repository logo](../brand/svipall.png): navy silhouettes, cream interwoven beard, terracotta accents and a single amber eye. Norse imagery supplies the concepts; the logo supplies their visual construction.

| Asset | README section | Meaning |
| --- | --- | --- |
| [classify-what-arrived.png](classify-what-arrived.png) | Judging what came back | HTTP status alone cannot distinguish an article, sign-in page and soft 404. Content integrity is a separate assessment. |
| [shapeshifter-identities.png](shapeshifter-identities.png) | Getting in | Different domain identities share internally coherent signals. Sessions persist until retirement; this does not depict rotation on every page. |
| [request-ladder.png](request-ladder.png) | How it works | Conditional tier order, successful return from any tier, learned domain starts and wall-dependent jumps/stops. |

## Technical grounding

- [Wall classifier](../../crates/svipall-core/src/classify.rs): `none`, `login` and `softnotfound` are real serialized values.
- [Quality](../../crates/svipall-core/src/quality/mod.rs): `full`, `partial` and `thin` describe delivered content. Labels do not remove documents or escalate the ladder.
- [Ladder](../../crates/svipall-core/src/ladder.rs) and [server](../../crates/svipall-mcp/src/server.rs): the diagram simplifies automatic mode. The configured tier cap and wall-specific routing still apply. The local solver/human line describes challenge handling, not a universal remedy for every failed request.
- [Sessions](../../crates/svipall-core/src/session.rs): cookies, machine identity and exit form a session; failures affect retirement.
- These illustrations make no competitor-exclusivity or performance claims. The README comparison retains its own sources and qualifications.

## Future vector source

Reuse the existing mascot SVG when reconstructing these as native vectors. Author diagram text and connectors as editable elements and simplify generated curves. Do not describe a PNG embedded in an SVG wrapper as vectorized artwork.

## Generation prompts

The edit targets were the preceding illustrated drafts. Their content and labels were retained while the logo became the controlling style reference.

### Classifier

REDESIGN THE ATTACHED CLASSIFICATION DIAGRAM TO MATCH THE ATTACHED LOGO'S FLAT VECTOR AESTHETIC.
Reference image 1 is the MASTER STYLE AND CHARACTER reference, the existing SVIPALL logo. Follow its graphic construction: large ink-navy solid silhouettes, warm cream negative spaces, a few terracotta planes, one small amber eye, sculpted smoothly interwoven geometric beard ribbons, broad angular pointed hat with rust knot band, strong angular cloak collar. Treat this logo as the final visual language, not as a source to turn into engraving.
Reference image 2 is the EDIT TARGET, the old classifier diagram. Preserve its MEANING and exact key labels but entirely replace the old drawing style. No old-paper appearance. No hand-drawn or woodcut strokes. No hatching, stippling, scratches, distressed outlines, tiny hair lines, grain, gradients, shadows, realism or medieval background scenery. This must look like professional vector brand illustration directly belonging to the reference logo family.
Create a wide approximately 2.5:1 supplemental README graphic. White clean background #FFFFFF. Left 22% features the existing mascot as an exact-looking flat vector bust, no staff, no realistic hands, preserve the recognizable face/hat/interlaced beard/collar proportions of reference 1. To its right, three equally sized simple flat geometric page silhouettes, with slightly clipped top-right corners and navy outlines. First page has terracotta heading bar and navy paragraph bars; second has a flat sign-in form with geometric user icon and two fields; third has large terracotta "404". Each page has top text exactly "HTTP 200". Under the pages, exact large clean monospaced labels: "none", "login", "softnotfound", each aligned with its corresponding page. A small heading over those three labels reads "wall_kind". Do not link the first page to full, the login to partial, or the 404 to thin: wall classification and content integrity are separate.
Bottom: a clean thin navy rule and centered exact text "Delivered content: full / partial / thin" then smaller "Quality labels. Every page kept." Remove all parchment, timber desk, compass, quill, decorative books, elaborate border. Use only the mascot, pages, labels and optional one simple flat interlace connecting accent. Retain ample white space around every element. Palette only #0B1A2B, #A7472C, #EAD9C4, #DF8D27 and white. Technical text simple crisp monospaced; no serif headline, no wordmark, no brand title. Clearly readable at 850px width. Produce the revised raster image with a convincingly clean SVG-like aesthetic.

### Request ladder

REDESIGN THE EXISTING FLOW DIAGRAM INTO THE EXACT FLAT VECTOR DESIGN LANGUAGE OF THE SVIPALL LOGO.
Reference 1 is the MASTER STYLE: existing logo, bold navy silhouette, geometric interwoven cream beard, pointed broad hat, rust collar planes, tiny amber eye. Its solid clean fills and controlled curves are the whole visual direction. Reference 2 is the EDIT TARGET: existing ladder illustration; preserve its real flow structure and words, completely replace the engraved illustrative treatment.
Create a wide horizontal supplemental README diagram around 2.5:1. PURE WHITE BACKGROUND. Five minimal geometric Nordic portal icons evenly spaced across the row, constructed with thick navy angular pillars and simple cream/terracotta flat lintels. One tiny interlace ornament per lintel at most. Absolutely no timber grain, hatching, mountains, grass, weathering, realistic stonework, stippling, paper texture, shadows, gradients or 3D. The portals should look like icons from the same vector family as the logo. Small simplified full-body version of the logo mascot at the first portal can carry a plain geometric staff, but the figure must be built from flat silhouette shapes with a cream interlaced beard and terracotta collar, never drawn like a storybook person. Leave the other portals mostly empty.
Exact top text "Escalate when needed". Exact large monospaced tier labels above the five portals left to right: "http", "browser", "stealth", "real", "warm". Connect each to the next with a simple amber right arrow. From EACH of the five portals draw a downward amber arrow to one common collection line below them; the http and warm branches MUST exist and point DOWN. Collection line leads to a flat open-book/document icon at bottom right with label "Markdown + metadata". This represents success at ANY tier, not completion of all tiers. Under collection line centered exact text "Stop when the page arrives". At bottom left, a minimal folded-map icon and label "Remember per domain". No arrow up from the success line into http.
Bottom small text beneath a simple thin rule:
"Wall classification can jump tiers or stop the ladder."
"Unresolved challenge: local solver / human dashboard"
Keep all labels uncluttered, with strong hierarchy, generous white space and safe margins. Use only navy #0B1A2B, cream #EAD9C4, terracotta #A7472C, amber #DF8D27, white. No wordmark, header banner or new title, no decoration unrelated to the flow. Polished precise SVG-like asset, raster output.

### Shapeshifter identities

REDESIGN THE SHAPESHIFTER DOCUMENTATION ILLUSTRATION IN THE EXACT FLAT VECTOR AESTHETIC OF THE EXISTING SVIPALL LOGO.
Reference 1 is the MASTER STYLE and the precise canonical character: a bold navy bust silhouette with wide angular pointed hat, rust hatband knotted at center, just ONE small amber eye, geometric cream beard made of a few broad beautifully interwoven ribbons, angular rust cloak collar. Follow its clean shape construction, limited flat fills and polished strong negative spaces. Reference 2 is the EDIT TARGET: old three-domain illustration. Keep the conceptual three-scene structure and exact captions, but replace every engraved stroke and old-book aesthetic.
Wide approximately 2.5:1 supplemental README graphic, PURE WHITE background. Three evenly spaced flat geometric portal/page frames, simple navy outlines and cream panels, each with minimal knot accent. Three large related mascot BUSTS across the foreground, not realistic full-body people, occupying roughly the lower half of the frames. FIRST bust faithfully matches reference 1's broad hat, single amber eye, bold navy silhouette, cream geometric braided beard and rust angular collar. SECOND bust is the SAME character transformed into a hooded scholar: angular navy hood, same single amber eye, same interwoven cream beard construction, small angular rust cloak clasp; clearly different silhouette. THIRD bust is the SAME character transformed into a travelling storyteller: folded asymmetrical navy cap with rust band, rust shoulder mantle, same single amber eye and cream interlaced beard. No realistic skin, individual hairs, wrinkles, stitches, fabric shading or slender curlicues. Use a few thick interwoven ribbons at the base linking the three busts, one restrained transition between shapes. Beards and garment edges use flat geometry of original logo, not stylized engraving.
Above frames exact large monospaced labels left to right: "docs.example", "shop.example", "news.example". Inside the frames behind the busts show restrained flat page layouts: docs has heading/paragraph bars, shop has a 3-column table, news has three plain article-card rectangles; no illustrations of boats/mountains/weapons. Enough whitespace so identity differences remain focal. No staffs, hands or unnecessary complex props.
Bottom three centered exact caption lines:
"One coherent identity per domain"
"TLS + headers + browser + behavior"
"Keep the session. Retire it when necessary."
Use crisp restrained monospace, readable at repository width. Only navy #0B1A2B, cream #EAD9C4, rust #A7472C, small amber #DF8D27 and white. No gradients, shadows, textures, paper grain, scratchiness, hatching, stippling, carved wood, 3D, realistic rendering, pseudo-runes or elaborate borders. No logo wordmark, no new headline or extra text. Finished aesthetic should appear designed in SVG alongside the exact supplied logo. Produce revised raster proposal.

