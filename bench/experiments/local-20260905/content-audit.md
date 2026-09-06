# Content audit of initial native recoveries

This is a limited manual inspection of saved responses, not a new scoring rule. The frozen
benchmark's expected strings remain unchanged for every arm. Matching a brand name can pass that
rule without returning the page's useful data, so the aggregate score needs this qualification.

| Saved sample | Inspection |
|---|---|
| `native-hard12-1`, G2, first visit | HTTP 200, Notion review title and 17,212 characters of review/product material |
| `native-hard12-1`, Idealista, first visit | HTTP 200, Madrid listing title and 21,918 characters including property listings and prices |
| `native-hard12-1`, Crunchbase, first visit | HTTP 200, the expected company profile and 15,810 characters including funding/profile information |
| `native-vendors8-1`, Home Depot, both visits | HTTP 200 and the brand match satisfy the frozen rule, but the 242/237-character responses contain a title, help text and speculation-rule JSON. They do not contain an appliance catalogue |
| `native-vendors8-2`, Home Depot, both visits | The same limitation repeats: 242 characters each, while the frozen rule again counts both as delivered |
| `native-vendors8-3`, Home Depot, both visits | First visit: 105 characters of JavaScript and the page title. Repeat: 242 characters. Neither contains an appliance catalogue, although both pass the frozen rule |
| `before-hard12-1`, Zillow, first visit | HTTP 200 and 666 characters: a search page explicitly reports zero matching homes and gives search tips. The new wall is a delivery-score regression, but the reference did not retrieve property listings in this sample |

The Home Depot score establishes neither complete content retrieval nor acceptance of all the
page's protected API calls. It must not be described as recovering the catalogue. The initial
native `vendors8` score of 7/8 includes this limited response.

The application's `quality=thin` label also appears on long, data-heavy listing/review pages in
these runs. That label alone cannot be used to declare all of those responses failures. Raw text,
expected content and reported walls remain available in each sample for inspection.
