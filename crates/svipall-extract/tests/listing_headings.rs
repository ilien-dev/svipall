use svipall_extract::{extract_markdown_opts, ExtractOpts};

fn markdown(html: &str, main_content_only: bool) -> String {
    extract_markdown_opts(
        html,
        &ExtractOpts {
            main_content_only,
            ..Default::default()
        },
    )
}

fn records(depth: usize) -> String {
    (1..=5)
        .map(|i| {
            format!(
                "<article>{}<h2><a href=\"https://records.test/items/{i}\">Research Engineer {i}</a></h2>{}\
                 <p>Institute {i} is recruiting an engineer to develop reliable research software. \
                 The role includes maintaining data pipelines, reviewing experimental results, \
                 documenting reproducible methods, and working with researchers to publish useful \
                 findings. This is a permanent position with a defined technical remit.</p></article>",
                "<div>".repeat(depth),
                "</div>".repeat(depth),
            )
        })
        .collect()
}

#[test]
fn neutral_wrappers_do_not_remove_listing_titles_or_item_links() {
    let direct = markdown(&format!("<main>{}</main>", records(0)), true);
    for i in 1..=5 {
        assert!(direct.contains(&format!(
            "[Research Engineer {i}](https://records.test/items/{i})"
        )));
        assert!(direct.contains(&format!("Institute {i} is recruiting")));
    }
    for depth in [1, 2, 6] {
        let html = format!("<main>{}</main>", records(depth));
        assert_eq!(
            markdown(&html, true),
            direct,
            "neutral wrapper depth {depth} changed the extracted records"
        );
        assert_eq!(markdown(&html, false), direct);
    }
}

#[test]
fn linked_headings_do_not_restore_navigation_or_related_rails() {
    let html = format!(
        "<main>{}<nav><div><h2><a href=\"/menu\">Navigation sentinel</a></h2></div></nav>\
         <div class=\"related\"><div><h2><a href=\"/related\">Related sentinel</a></h2></div></div>\
         <aside><div><h2><a href=\"/aside\">Sidebar sentinel</a></h2></div></aside></main>",
        records(0),
    );
    let out = markdown(&html, true);
    assert!(out.contains("Institute 5 is recruiting"));
    for sentinel in [
        "Navigation sentinel",
        "Related sentinel",
        "Sidebar sentinel",
    ] {
        assert!(!out.contains(sentinel), "page furniture survived: {out}");
        assert!(markdown(&html, false).contains(sentinel));
    }
}

#[test]
fn navigation_roles_on_neutral_divs_are_not_record_headings() {
    for role in ["navigation", "menu", "menubar"] {
        let html = format!(
            "<main>{}<div role=\"{role}\"><div><h2><a href=\"/menu\">Role sentinel</a></h2></div></div></main>",
            records(0),
        );
        let out = markdown(&html, true);
        assert!(out.contains("Institute 5 is recruiting"));
        assert!(!out.contains("Role sentinel"), "{role} survived: {out}");
    }
}

#[test]
fn hidden_record_headings_stay_hidden_in_both_extraction_modes() {
    let html = format!(
        "<main>{}<article>\
         <div hidden><h2><a href=\"/hidden\">Hidden sentinel</a></h2></div>\
         <div aria-hidden=\"true\"><h2><a href=\"/aria\">Aria sentinel</a></h2></div>\
         <div><h2 hidden><a href=\"/inner\">Inner sentinel</a></h2></div>\
         <p>This visible description must survive even when the associated heading is hidden.</p>\
         </article></main>",
        records(0),
    );
    for main_only in [true, false] {
        let out = markdown(&html, main_only);
        assert!(out.contains("This visible description must survive"));
        for sentinel in ["Hidden sentinel", "Aria sentinel", "Inner sentinel"] {
            assert!(!out.contains(sentinel), "hidden heading survived: {out}");
        }
    }
}
