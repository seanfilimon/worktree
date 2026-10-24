//! License detection: `SPDX-License-Identifier:` headers and full-text
//! fingerprints of common license bodies.
//!
//! Full-text matching normalizes the input (ASCII-lowercase, every run of
//! non-alphanumeric characters collapsed to one space, British spellings and
//! `https` folded) and then looks for distinctive phrases. Phrases are chosen
//! so that re-wrapped, re-indented, comment-prefixed or lightly edited copies
//! still match, while texts that merely *mention* another license (the GPL
//! referring to the LGPL, say) do not.

use super::is_binary;

/// Number of leading lines searched for an SPDX header.
const SPDX_HEADER_LINES: usize = 20;

/// Upper bound on the bytes normalized for full-text matching. The longest
/// supported license body (GPL-3.0) is ~35 KiB.
const MAX_LICENSE_TEXT: usize = 1024 * 1024;

/// Detect a license; see [`super::detect_license`].
pub(super) fn detect(data: &[u8]) -> Option<String> {
    if data.is_empty() || is_binary(data) {
        return None;
    }
    if let Some(expr) = spdx_header(data) {
        return Some(expr);
    }
    let text = String::from_utf8_lossy(&data[..data.len().min(MAX_LICENSE_TEXT)]);
    fingerprint(&normalize(&text))
}

// ---------------------------------------------------------------------------
// SPDX headers
// ---------------------------------------------------------------------------

const SPDX_TAG: &str = "spdx-license-identifier";

/// Find a valid `SPDX-License-Identifier:` expression in the first
/// [`SPDX_HEADER_LINES`] lines, regardless of comment syntax.
fn spdx_header(data: &[u8]) -> Option<String> {
    data.split(|&b| b == b'\n')
        .take(SPDX_HEADER_LINES)
        .find_map(|line| parse_spdx_line(&String::from_utf8_lossy(line)))
}

fn parse_spdx_line(line: &str) -> Option<String> {
    // ASCII lowercasing preserves byte offsets, so `idx` indexes `line` too.
    let idx = line.to_ascii_lowercase().find(SPDX_TAG)?;
    let rest = line[idx + SPDX_TAG.len()..].trim_start();
    let rest = rest.strip_prefix(':')?;
    // Keep the longest prefix of expression characters; this drops comment
    // terminators such as `*/`, `-->`, `*)` or closing quotes.
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || " \t.-+:()".contains(c)))
        .unwrap_or(rest.len());
    let expr = rest[..end].trim().trim_end_matches(['-', ' ', '\t']);
    normalize_spdx_expression(expr)
}

/// Validate an SPDX license expression and return it with whitespace
/// collapsed and operators upper-cased. `None` if it is not well-formed.
fn normalize_spdx_expression(expr: &str) -> Option<String> {
    let spaced = expr.replace('(', " ( ").replace(')', " ) ");
    let mut out = String::new();
    let mut depth = 0usize;
    // True when the next token must be an operand (license id or "(").
    let mut expect_operand = true;
    let mut after_with = false;

    for token in spaced.split_whitespace() {
        let upper = token.to_ascii_uppercase();
        match upper.as_str() {
            "(" => {
                if !expect_operand || after_with {
                    return None;
                }
                depth += 1;
                out.push('(');
                continue;
            }
            ")" => {
                if expect_operand || depth == 0 {
                    return None;
                }
                depth -= 1;
                out.push(')');
                continue;
            }
            "AND" | "OR" | "WITH" => {
                if expect_operand {
                    return None;
                }
                after_with = upper == "WITH";
                expect_operand = true;
                push_word(&mut out, &upper);
            }
            _ => {
                if !expect_operand || !is_license_ref(token) {
                    return None;
                }
                expect_operand = false;
                after_with = false;
                push_word(&mut out, token);
            }
        }
    }
    (!expect_operand && depth == 0).then_some(out)
}

fn push_word(out: &mut String, word: &str) {
    if !(out.is_empty() || out.ends_with('(')) {
        out.push(' ');
    }
    out.push_str(word);
}

/// `idstring` per the SPDX grammar, optionally `DocumentRef-x:LicenseRef-y`,
/// optionally with a trailing `+` ("or later").
fn is_license_ref(token: &str) -> bool {
    let token = token.strip_suffix('+').unwrap_or(token);
    let mut parts = token.split(':');
    let valid_part = |p: &str| {
        p.starts_with(|c: char| c.is_ascii_alphanumeric())
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    };
    match (parts.next(), parts.next(), parts.next()) {
        (Some(id), None, None) => valid_part(id),
        (Some(doc), Some(id), None) => {
            doc.starts_with("DocumentRef-") && valid_part(doc) && valid_part(id)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Full-text fingerprints
// ---------------------------------------------------------------------------

/// Normalize license text for phrase matching. The result starts and ends
/// with a space so phrases can be matched on word boundaries.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len() / 2 + 2);
    out.push(' ');
    let mut word = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            word.push(c.to_ascii_lowercase());
        } else if !word.is_empty() {
            push_normalized_word(&mut out, &word);
            word.clear();
        }
    }
    if !word.is_empty() {
        push_normalized_word(&mut out, &word);
    }
    out
}

fn push_normalized_word(out: &mut String, word: &str) {
    let folded = match word {
        "licence" => "license",
        "licences" => "licenses",
        "licenced" => "licensed",
        "https" => "http",
        "acknowledgment" => "acknowledgement",
        other => other,
    };
    out.push_str(folded);
    out.push(' ');
}

/// Whether normalized `text` contains normalized `phrase` as whole words.
fn has(text: &str, phrase: &str) -> bool {
    find(text, phrase).is_some()
}

/// Byte offset of the first whole-word occurrence of `phrase` in `text`.
fn find(text: &str, phrase: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    text.match_indices(phrase).map(|(i, _)| i).find(|&i| {
        let end = i + phrase.len();
        (i == 0 || bytes[i - 1] == b' ') && (end == bytes.len() || bytes[end] == b' ')
    })
}

/// A license is matched when *any* alternative matches, and an alternative
/// matches when *all* of its phrases are present.
struct Fingerprint {
    id: &'static str,
    /// GNU licenses get an `-only` / `-or-later` suffix.
    gnu: bool,
    alternatives: &'static [&'static [&'static str]],
}

const BSD_REDISTRIBUTION: &str =
    "redistribution and use in source and binary forms with or without modification are permitted provided that";

/// Checked in order; more specific texts come before texts they contain or
/// mention (AGPL/LGPL before GPL, BSD-4 before BSD-3 before BSD-2).
static FINGERPRINTS: &[Fingerprint] = &[
    Fingerprint {
        id: "AGPL-3.0",
        gnu: true,
        alternatives: &[
            &["gnu affero general public license version 3 19 november 2007"],
            &["terms of the gnu affero general public license as published by the free software foundation"],
            &["terms of the gnu affero general public license version 3 as published by"],
        ],
    },
    Fingerprint {
        id: "LGPL-3.0",
        gnu: true,
        alternatives: &[
            &["gnu lesser general public license version 3 29 june 2007"],
            &["terms of the gnu lesser general public license as published by the free software foundation either version 3"],
            &["terms of the gnu lesser general public license version 3 as published by"],
        ],
    },
    Fingerprint {
        id: "LGPL-2.1",
        gnu: true,
        alternatives: &[
            &["gnu lesser general public license version 2 1 february 1999"],
            &["terms of the gnu lesser general public license as published by the free software foundation either version 2 1"],
            &["terms of the gnu lesser general public license version 2 1 as published by"],
        ],
    },
    Fingerprint {
        id: "GPL-3.0",
        gnu: true,
        alternatives: &[
            &["gnu general public license version 3 29 june 2007"],
            &["terms of the gnu general public license as published by the free software foundation either version 3"],
            &["terms of the gnu general public license version 3 as published by"],
        ],
    },
    Fingerprint {
        id: "GPL-2.0",
        gnu: true,
        alternatives: &[
            &["gnu general public license version 2 june 1991"],
            &["terms of the gnu general public license as published by the free software foundation either version 2"],
            &["terms of the gnu general public license version 2 as published by"],
        ],
    },
    Fingerprint {
        id: "Apache-2.0",
        gnu: false,
        alternatives: &[&["apache license version 2 0"], &["apache license 2 0"]],
    },
    Fingerprint {
        id: "MPL-2.0",
        gnu: false,
        alternatives: &[
            &["mozilla public license version 2 0"],
            &["mozilla public license v 2 0"],
        ],
    },
    Fingerprint {
        id: "BSL-1.0",
        gnu: false,
        alternatives: &[
            &["boost software license version 1 0"],
            &["permission is hereby granted free of charge to any person or organization obtaining a copy of the software and accompanying documentation covered by this license"],
        ],
    },
    Fingerprint {
        id: "MIT",
        gnu: false,
        alternatives: &[&[
            "permission is hereby granted free of charge to any person obtaining a copy of this software",
            "the above copyright notice and this permission notice shall be included in all copies or substantial portions of the software",
        ]],
    },
    Fingerprint {
        id: "ISC",
        gnu: false,
        alternatives: &[&[
            "distribute this software for any purpose with or without fee is hereby granted",
            "provided that the above copyright notice and this permission notice appear in all copies",
        ]],
    },
    Fingerprint {
        id: "BSD-4-Clause",
        gnu: false,
        alternatives: &[&[
            BSD_REDISTRIBUTION,
            "all advertising materials mentioning features or use of this software must display the following acknowledgement",
        ]],
    },
    Fingerprint {
        id: "BSD-3-Clause",
        gnu: false,
        alternatives: &[&[
            BSD_REDISTRIBUTION,
            "used to endorse or promote products derived from this software",
        ]],
    },
    Fingerprint {
        id: "BSD-2-Clause",
        gnu: false,
        alternatives: &[&[
            BSD_REDISTRIBUTION,
            "redistributions in binary form must reproduce the above copyright notice",
        ]],
    },
    Fingerprint {
        id: "Unlicense",
        gnu: false,
        alternatives: &[&["this is free and unencumbered software released into the public domain"]],
    },
    Fingerprint {
        id: "CC0-1.0",
        gnu: false,
        alternatives: &[
            &["cc0 1 0 universal"],
            &["creativecommons org publicdomain zero 1 0"],
        ],
    },
];

fn fingerprint(norm: &str) -> Option<String> {
    let fp = FINGERPRINTS.iter().find(|fp| {
        fp.alternatives
            .iter()
            .any(|phrases| phrases.iter().all(|p| has(norm, p)))
    })?;
    if !fp.gnu {
        return Some(fp.id.to_string());
    }
    let suffix = if gnu_or_later(norm) {
        "-or-later"
    } else {
        "-only"
    };
    Some(format!("{}{suffix}", fp.id))
}

/// Whether a GNU license grant includes "or (at your option) any later
/// version". The canonical license texts contain that phrase in their
/// "How to Apply These Terms" appendix as an *example* notice, so
/// occurrences inside the appendix do not count.
fn gnu_or_later(norm: &str) -> bool {
    let Some(grant) = find(norm, "at your option any later version") else {
        return false;
    };
    match find(norm, "how to apply these terms to your new") {
        Some(appendix) => grant < appendix,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::super::detect_license;

    fn detect(s: &str) -> Option<String> {
        detect_license(s.as_bytes())
    }

    const MIT: &str = "MIT License\n\nCopyright (c) 2024 Example Corp\n\n\
        Permission is hereby granted, free of charge, to any person obtaining a copy\n\
        of this software and associated documentation files (the \"Software\"), to deal\n\
        in the Software without restriction, including without limitation the rights\n\
        to use, copy, modify, merge, publish, distribute, sublicense, and/or sell\n\
        copies of the Software, and to permit persons to whom the Software is\n\
        furnished to do so, subject to the following conditions:\n\n\
        The above copyright notice and this permission notice shall be included in all\n\
        copies or substantial portions of the Software.\n\n\
        THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND.\n";

    const BSD_PREAMBLE: &str = "Copyright (c) 2020, The Authors\nAll rights reserved.\n\n\
        Redistribution and use in source and binary forms, with or without\n\
        modification, are permitted provided that the following conditions are met:\n\n\
        1. Redistributions of source code must retain the above copyright notice, this\n\
           list of conditions and the following disclaimer.\n\n\
        2. Redistributions in binary form must reproduce the above copyright notice,\n\
           this list of conditions and the following disclaimer in the documentation\n\
           and/or other materials provided with the distribution.\n";

    #[test]
    fn mit_full_text() {
        assert_eq!(detect(MIT).as_deref(), Some("MIT"));
    }

    #[test]
    fn mit_near_variant_rewrapped_and_commented() {
        let variant: String = MIT
            .to_uppercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .chunks(7)
            .map(|w| format!("// {}\n", w.join("   ")))
            .collect();
        assert_eq!(detect(&variant).as_deref(), Some("MIT"));
    }

    #[test]
    fn apache_2_full_text_header() {
        let text = "                                 Apache License\n\
                                   Version 2.0, January 2004\n\
                                http://www.apache.org/licenses/\n\n\
            TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION\n";
        assert_eq!(detect(text).as_deref(), Some("Apache-2.0"));
    }

    #[test]
    fn apache_2_notice() {
        let text = "Licensed under the Apache License, Version 2.0 (the \"License\");\n\
            you may not use this file except in compliance with the License.";
        assert_eq!(detect(text).as_deref(), Some("Apache-2.0"));
    }

    #[test]
    fn gpl_3_full_text_is_only() {
        let text = "                    GNU GENERAL PUBLIC LICENSE\n\
                               Version 3, 29 June 2007\n\n\
            Copyright (C) 2007 Free Software Foundation, Inc. <https://fsf.org/>\n\
            Everyone is permitted to copy and distribute verbatim copies\n\
            of this license document, but changing it is not allowed.\n\n\
            If your program is a subroutine library, you may consider it more useful\n\
            to permit linking proprietary applications with the library.  If this is\n\
            what you want to do, use the GNU Lesser General Public License instead.\n\n\
                       How to Apply These Terms to Your New Programs\n\n\
            This program is free software: you can redistribute it and/or modify\n\
            it under the terms of the GNU General Public License as published by\n\
            the Free Software Foundation, either version 3 of the License, or\n\
            (at your option) any later version.\n";
        assert_eq!(detect(text).as_deref(), Some("GPL-3.0-only"));
    }

    #[test]
    fn gpl_3_or_later_notice() {
        let text = "This program is free software: you can redistribute it and/or modify\n\
            it under the terms of the GNU General Public License as published by\n\
            the Free Software Foundation, either version 3 of the License, or\n\
            (at your option) any later version.\n";
        assert_eq!(detect(text).as_deref(), Some("GPL-3.0-or-later"));
    }

    #[test]
    fn gpl_2_full_text_is_only() {
        let text = "\t\t    GNU GENERAL PUBLIC LICENSE\n\t\t       Version 2, June 1991\n\n\
             Copyright (C) 1989, 1991 Free Software Foundation, Inc.\n\n\
            \t    How to Apply These Terms to Your New Programs\n\n\
                This program is free software; you can redistribute it and/or modify\n\
                it under the terms of the GNU General Public License as published by\n\
                the Free Software Foundation; either version 2 of the License, or\n\
                (at your option) any later version.\n";
        assert_eq!(detect(text).as_deref(), Some("GPL-2.0-only"));
    }

    #[test]
    fn gpl_2_or_later_notice() {
        let text = "# This program is free software; you can redistribute it and/or modify\n\
            # it under the terms of the GNU General Public License as published by\n\
            # the Free Software Foundation; either version 2 of the License, or\n\
            # (at your option) any later version.\n";
        assert_eq!(detect(text).as_deref(), Some("GPL-2.0-or-later"));
    }

    #[test]
    fn gpl_2_only_notice_linux_style() {
        let text = "This program is free software; you can redistribute it and/or modify it\n\
            under the terms of the GNU General Public License version 2 as published by\n\
            the Free Software Foundation.\n";
        assert_eq!(detect(text).as_deref(), Some("GPL-2.0-only"));
    }

    #[test]
    fn lgpl_2_1_full_text() {
        let text = "\t\t  GNU LESSER GENERAL PUBLIC LICENSE\n\
            \t\t       Version 2.1, February 1999\n\n\
             Copyright (C) 1991, 1999 Free Software Foundation, Inc.\n";
        assert_eq!(detect(text).as_deref(), Some("LGPL-2.1-only"));
    }

    #[test]
    fn lgpl_2_1_or_later_notice() {
        let text = "This library is free software; you can redistribute it and/or\n\
            modify it under the terms of the GNU Lesser General Public\n\
            License as published by the Free Software Foundation; either\n\
            version 2.1 of the License, or (at your option) any later version.\n";
        assert_eq!(detect(text).as_deref(), Some("LGPL-2.1-or-later"));
    }

    #[test]
    fn lgpl_3_full_text_mentions_gpl_but_is_lgpl() {
        let text = "                   GNU LESSER GENERAL PUBLIC LICENSE\n\
                                   Version 3, 29 June 2007\n\n\
              This version of the GNU Lesser General Public License incorporates\n\
            the terms and conditions of version 3 of the GNU General Public\n\
            License, supplemented by the additional permissions listed below.\n";
        assert_eq!(detect(text).as_deref(), Some("LGPL-3.0-only"));
    }

    #[test]
    fn agpl_3_full_text() {
        let text = "                    GNU AFFERO GENERAL PUBLIC LICENSE\n\
                                   Version 3, 19 November 2007\n\n\
             Copyright (C) 2007 Free Software Foundation, Inc. <https://fsf.org/>\n";
        assert_eq!(detect(text).as_deref(), Some("AGPL-3.0-only"));
    }

    #[test]
    fn agpl_3_or_later_notice() {
        let text = "it under the terms of the GNU Affero General Public License as published\n\
            by the Free Software Foundation, either version 3 of the License, or\n\
            (at your option) any later version.";
        assert_eq!(detect(text).as_deref(), Some("AGPL-3.0-or-later"));
    }

    #[test]
    fn bsd_2_clause() {
        let text = format!("{BSD_PREAMBLE}\nTHIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS \"AS IS\"\n");
        assert_eq!(detect(&text).as_deref(), Some("BSD-2-Clause"));
    }

    #[test]
    fn bsd_3_clause() {
        let text = format!(
            "{BSD_PREAMBLE}\n3. Neither the name of the copyright holder nor the names of its\n   \
             contributors may be used to endorse or promote products derived from\n   \
             this software without specific prior written permission.\n"
        );
        assert_eq!(detect(&text).as_deref(), Some("BSD-3-Clause"));
    }

    #[test]
    fn bsd_4_clause() {
        let text = format!(
            "{BSD_PREAMBLE}\n3. All advertising materials mentioning features or use of this software\n   \
             must display the following acknowledgment: This product includes software\n\
             4. Neither the name of the University nor the names of its contributors\n   \
             may be used to endorse or promote products derived from this software\n"
        );
        assert_eq!(detect(&text).as_deref(), Some("BSD-4-Clause"));
    }

    #[test]
    fn mpl_2_full_text_and_notice() {
        assert_eq!(
            detect("Mozilla Public License Version 2.0\n==================================\n")
                .as_deref(),
            Some("MPL-2.0")
        );
        let notice = "This Source Code Form is subject to the terms of the Mozilla Public\n\
            License, v. 2.0. If a copy of the MPL was not distributed with this\n\
            file, You can obtain one at http://mozilla.org/MPL/2.0/.";
        assert_eq!(detect(notice).as_deref(), Some("MPL-2.0"));
    }

    #[test]
    fn isc() {
        let text =
            "ISC License\n\nCopyright (c) 2004-2010 by Internet Systems Consortium, Inc.\n\n\
            Permission to use, copy, modify, and/or distribute this software for any\n\
            purpose with or without fee is hereby granted, provided that the above\n\
            copyright notice and this permission notice appear in all copies.\n";
        assert_eq!(detect(text).as_deref(), Some("ISC"));
    }

    #[test]
    fn unlicense() {
        let text = "This is free and unencumbered software released into the public domain.\n\n\
            Anyone is free to copy, modify, publish, use, compile, sell, or\n\
            distribute this software.\n";
        assert_eq!(detect(text).as_deref(), Some("Unlicense"));
    }

    #[test]
    fn cc0() {
        let text = "Creative Commons Legal Code\n\nCC0 1.0 Universal\n\n\
            CREATIVE COMMONS CORPORATION IS NOT A LAW FIRM\n";
        assert_eq!(detect(text).as_deref(), Some("CC0-1.0"));
        let url = "See https://creativecommons.org/publicdomain/zero/1.0/ for details.";
        assert_eq!(detect(url).as_deref(), Some("CC0-1.0"));
    }

    #[test]
    fn boost() {
        let text = "Boost Software License - Version 1.0 - August 17th, 2003\n\n\
            Permission is hereby granted, free of charge, to any person or organization\n\
            obtaining a copy of the software and accompanying documentation covered by\n\
            this license (the \"Software\") to use, reproduce, display, distribute,\n";
        assert_eq!(detect(text).as_deref(), Some("BSL-1.0"));
        // The grant alone, without the title, is distinctive enough.
        let grant = &text[text.find("Permission").unwrap_or(0)..];
        assert_eq!(detect(grant).as_deref(), Some("BSL-1.0"));
    }

    #[test]
    fn british_spelling_and_https_are_folded() {
        let text = "Licenced under the Apache Licence, Version 2.0; see https://www.apache.org/";
        assert_eq!(detect(text).as_deref(), Some("Apache-2.0"));
    }

    #[test]
    fn unknown_text_is_none() {
        assert_eq!(detect("All rights reserved. Do not distribute.\n"), None);
        assert_eq!(detect("fn main() { println!(\"hello\"); }\n"), None);
        assert_eq!(detect(""), None);
    }

    #[test]
    fn mere_mention_of_gpl_is_not_a_license() {
        assert_eq!(
            detect("We considered the GNU General Public License.\n"),
            None
        );
    }

    #[test]
    fn derived_license_referencing_gpl_is_not_gpl() {
        let text =
            "                    ACME PUBLIC LICENSE\n                        Version 1.0\n\n\
            This license is based on the GNU General Public License, version 2\n\
            (GPL-2.0), with one additional requirement.\n\n\
            This section constitutes the sole additional requirement imposed by\n\
            this License beyond the terms of the GNU General Public License,\n\
            version 2.\n";
        assert_eq!(detect(text), None);
    }

    #[test]
    fn binary_is_none() {
        let mut data = b"SPDX-License-Identifier: MIT\n".to_vec();
        data.push(0);
        assert_eq!(detect_license(&data), None);
    }

    #[test]
    fn spdx_header_comment_styles() {
        let cases = [
            "// SPDX-License-Identifier: MIT\nfn main() {}\n",
            "# SPDX-License-Identifier: MIT\n",
            "/* SPDX-License-Identifier: MIT */\n",
            "<!-- SPDX-License-Identifier: MIT -->\n",
            "-- SPDX-License-Identifier: MIT\n",
            "(* SPDX-License-Identifier: MIT *)\n",
            ";; SPDX-License-Identifier:MIT\n",
            " * spdx-license-identifier: MIT\r\n",
            "\"\"\"SPDX-License-Identifier: MIT\"\"\"\n",
        ];
        for case in cases {
            assert_eq!(detect(case).as_deref(), Some("MIT"), "case: {case:?}");
        }
    }

    #[test]
    fn spdx_expressions_are_returned_whole() {
        let cases = [
            (
                "// SPDX-License-Identifier: MIT OR Apache-2.0\n",
                "MIT OR Apache-2.0",
            ),
            (
                "# SPDX-License-Identifier:   MIT   or   Apache-2.0  \n",
                "MIT OR Apache-2.0",
            ),
            (
                "// SPDX-License-Identifier: (MIT OR Apache-2.0) AND BSD-3-Clause\n",
                "(MIT OR Apache-2.0) AND BSD-3-Clause",
            ),
            (
                "/* SPDX-License-Identifier: GPL-2.0-only WITH Classpath-exception-2.0 */",
                "GPL-2.0-only WITH Classpath-exception-2.0",
            ),
            ("# SPDX-License-Identifier: GPL-2.0+\n", "GPL-2.0+"),
            (
                "// SPDX-License-Identifier: LicenseRef-W0rkTree-Public-License-1.0\n",
                "LicenseRef-W0rkTree-Public-License-1.0",
            ),
            (
                "// SPDX-License-Identifier: DocumentRef-spdx-tool-1.2:LicenseRef-MIT-Style-2\n",
                "DocumentRef-spdx-tool-1.2:LicenseRef-MIT-Style-2",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(detect(input).as_deref(), Some(want), "input: {input:?}");
        }
    }

    #[test]
    fn spdx_header_beats_body_text() {
        let text = format!("// SPDX-License-Identifier: Apache-2.0\n{MIT}");
        assert_eq!(detect(&text).as_deref(), Some("Apache-2.0"));
    }

    #[test]
    fn spdx_header_only_in_first_lines() {
        let mut text = "line\n".repeat(25);
        text.push_str("// SPDX-License-Identifier: MIT\n");
        assert_eq!(detect(&text), None);
        let mut text = "line\n".repeat(19);
        text.push_str("// SPDX-License-Identifier: MIT\n");
        assert_eq!(detect(&text).as_deref(), Some("MIT"));
    }

    #[test]
    fn malformed_spdx_expressions_are_rejected() {
        for bad in [
            "// SPDX-License-Identifier:\n",
            "// SPDX-License-Identifier: MIT OR\n",
            "// SPDX-License-Identifier: OR MIT\n",
            "// SPDX-License-Identifier: (MIT\n",
            "// SPDX-License-Identifier: MIT Apache-2.0\n",
            "// SPDX-License-Identifier MIT\n",
            "// SPDX-License-Identifier: MIT WITH (Foo)\n",
        ] {
            assert_eq!(detect(bad), None, "input: {bad:?}");
        }
    }

    #[test]
    fn malformed_header_falls_back_to_body() {
        let text = format!("// SPDX-License-Identifier: OR\n{MIT}");
        assert_eq!(detect(&text).as_deref(), Some("MIT"));
    }
}
