//! LaTeX → Unicode：Word 与纯文本路径用的轻量符号转换。

/// 常用符号表（子集，够覆盖一般文档；未命中时保留原样）。
const SYMBOLS: &[(&str, &str)] = &[
    ("\\alpha", "α"), ("\\beta", "β"), ("\\gamma", "γ"), ("\\delta", "δ"),
    ("\\epsilon", "ε"), ("\\varepsilon", "ε"), ("\\zeta", "ζ"), ("\\eta", "η"),
    ("\\theta", "θ"), ("\\iota", "ι"), ("\\kappa", "κ"), ("\\lambda", "λ"),
    ("\\mu", "μ"), ("\\nu", "ν"), ("\\xi", "ξ"), ("\\pi", "π"), ("\\rho", "ρ"),
    ("\\sigma", "σ"), ("\\tau", "τ"), ("\\phi", "φ"), ("\\varphi", "φ"),
    ("\\chi", "χ"), ("\\psi", "ψ"), ("\\omega", "ω"),
    ("\\Gamma", "Γ"), ("\\Delta", "Δ"), ("\\Theta", "Θ"), ("\\Lambda", "Λ"),
    ("\\Xi", "Ξ"), ("\\Pi", "Π"), ("\\Sigma", "Σ"), ("\\Phi", "Φ"),
    ("\\Psi", "Ψ"), ("\\Omega", "Ω"),
    ("\\times", "×"), ("\\div", "÷"), ("\\pm", "±"), ("\\mp", "∓"),
    ("\\cdot", "·"), ("\\cdots", "⋯"), ("\\ldots", "…"), ("\\dots", "…"),
    ("\\leq", "≤"), ("\\le", "≤"), ("\\geq", "≥"), ("\\ge", "≥"),
    ("\\neq", "≠"), ("\\ne", "≠"), ("\\approx", "≈"), ("\\equiv", "≡"),
    ("\\sim", "∼"), ("\\propto", "∝"), ("\\infty", "∞"),
    ("\\in", "∈"), ("\\notin", "∉"), ("\\subset", "⊂"), ("\\subseteq", "⊆"),
    ("\\supset", "⊃"), ("\\cup", "∪"), ("\\cap", "∩"), ("\\emptyset", "∅"),
    ("\\forall", "∀"), ("\\exists", "∃"), ("\\neg", "¬"), ("\\land", "∧"),
    ("\\lor", "∨"), ("\\Rightarrow", "⇒"), ("\\Leftarrow", "⇐"),
    ("\\Leftrightarrow", "⇔"), ("\\rightarrow", "→"), ("\\to", "→"),
    ("\\mapsto", "↦"), ("\\partial", "∂"), ("\\nabla", "∇"),
    ("\\sqrt", "√"), ("\\sum", "∑"), ("\\prod", "∏"), ("\\int", "∫"),
    ("\\iint", "∬"), ("\\iiint", "∭"), ("\\oint", "∮"),
    ("\\degree", "°"), ("\\circ", "∘"), ("\\bullet", "•"),
    ("\\perp", "⊥"), ("\\parallel", "∥"), ("\\angle", "∠"),
    ("\\prime", "′"), ("\\hbar", "ℏ"), ("\\ell", "ℓ"),
    ("\\mathbb{R}", "ℝ"), ("\\mathbb{N}", "ℕ"), ("\\mathbb{Z}", "ℤ"),
    ("\\mathbb{Q}", "ℚ"), ("\\mathbb{C}", "ℂ"),
    ("\\triangle", "△"), ("\\because", "∵"), ("\\therefore", "∴"),
    ("\\cong", "≌"), ("\\odot", "⊙"), ("\\equiv", "≡"),
    ("\\gt", ">"), ("\\lt", "<"), ("\\geqslant", "≥"), ("\\leqslant", "≤"),
    ("\\angle", "∠"), ("\\perp", "⊥"),
];

/// 将一段类 LaTeX 文本转为 Unicode 近似。分式/上下标退化为线性形式。
pub fn to_unicode(input: &str) -> String {
    let mut s = strip_delims(input);
    s = unwrap_style_commands(s);
    // 角度：90^{\circ} → 90°
    s = s.replace("^{\\circ}", "°").replace("^\\circ", "°");

    // \frac{a}{b} → (a)/(b)
    while let Some(pos) = s.find("\\frac") {
        let after = &s[pos + 5..];
        let Some((num, rest1)) = take_brace(after) else { break };
        let Some((den, rest2)) = take_brace(rest1) else { break };
        let replacement = format!("({num})/({den})");
        s = format!("{}{replacement}{}", &s[..pos], rest2);
    }

    // ^{x} / _{x} → 上标/下标
    s = replace_scripts(&s, '^', superscript);
    s = replace_scripts(&s, '_', subscript);

    // 命名符号
    for (cmd, uni) in SYMBOLS {
        if s.contains(cmd) {
            s = s.replace(cmd, uni);
        }
    }

    // 残余花括号与未知命令的反斜杠清理（已知符号已在上面替换）
    s.replace(['{', '}', '\\'], "")
}

/// 去掉只影响字形的命令外壳：`\text{三角}` → `三角`，`\left(` → `(`。
fn unwrap_style_commands(mut s: String) -> String {
    const WRAPPERS: &[&str] = &[
        "\\text", "\\textrm", "\\mathrm", "\\mathbf", "\\mathit",
        "\\underline", "\\overline", "\\vec", "\\hat", "\\bar",
        "\\left", "\\right", "\\displaystyle",
    ];
    for cmd in WRAPPERS {
        while let Some(pos) = s.find(cmd) {
            let after = s[pos + cmd.len()..].to_string();
            if let Some((inner, rest)) = take_brace(&after) {
                let mut next = String::with_capacity(s.len());
                next.push_str(&s[..pos]);
                next.push_str(&inner);
                next.push_str(rest);
                s = next;
            } else {
                let mut next = String::with_capacity(s.len());
                next.push_str(&s[..pos]);
                next.push_str(&after);
                s = next;
            }
        }
    }
    s
}

fn strip_delims(s: &str) -> String {
    let t = s.trim();
    let t = t
        .strip_prefix("$$")
        .and_then(|x| x.strip_suffix("$$"))
        .or_else(|| t.strip_prefix("$$").and_then(|x| x.strip_suffix("$$")))
        .unwrap_or(t);
    let t = t
        .strip_prefix('$')
        .and_then(|x| x.strip_suffix('$'))
        .unwrap_or(t);
    let t = t
        .strip_prefix("\\[")
        .and_then(|x| x.strip_suffix("\\]"))
        .unwrap_or(t);
    let t = t
        .strip_prefix("\\(")
        .and_then(|x| x.strip_suffix("\\)"))
        .unwrap_or(t);
    t.trim().to_string()
}

fn take_brace(s: &str) -> Option<(String, &str)> {
    let rest = s.trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in rest.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((rest[1..i].to_string(), &rest[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn superscript(c: char) -> String {
    match c {
        '0' => "⁰".into(), '1' => "¹".into(), '2' => "²".into(), '3' => "³".into(),
        '4' => "⁴".into(), '5' => "⁵".into(), '6' => "⁶".into(), '7' => "⁷".into(),
        '8' => "⁸".into(), '9' => "⁹".into(), '+' => "⁺".into(), '-' => "⁻".into(),
        'n' => "ⁿ".into(), 'i' => "ⁱ".into(),
        _ => c.to_string(),
    }
}

fn subscript(c: char) -> String {
    match c {
        '0' => "₀".into(), '1' => "₁".into(), '2' => "₂".into(), '3' => "₃".into(),
        '4' => "₄".into(), '5' => "₅".into(), '6' => "₆".into(), '7' => "₇".into(),
        '8' => "₈".into(), '9' => "₉".into(), '+' => "₊".into(), '-' => "₋".into(),
        _ => c.to_string(),
    }
}

fn replace_scripts(s: &str, marker: char, conv: fn(char) -> String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // 保住转义命令（如 \times），原样搬移
            out.push(c);
            if let Some(next) = chars.next() {
                out.push(next);
            }
            continue;
        }
        if c == marker {
            if chars.peek() == Some(&'{') {
                chars.next();
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                    out.push_str(&conv(inner));
                }
            } else if let Some(inner) = chars.next() {
                if inner == '\\' {
                    // ^\cmd：保原样
                    out.push(marker);
                    out.push('\\');
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                } else {
                    out.push_str(&conv(inner));
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_basic_symbols() {
        assert_eq!(to_unicode("$a \\times b$"), "a × b");
        assert_eq!(to_unicode("$$\\alpha + \\beta$$"), "α + β");
    }

    #[test]
    fn converts_frac_and_scripts() {
        assert_eq!(to_unicode("\\frac{1}{2}"), "(1)/(2)");
        assert_eq!(to_unicode("x^{2} + y_1"), "x² + y₁");
    }

    #[test]
    fn unwraps_style_and_geometry() {
        assert_eq!(to_unicode("$\\triangle ABC$"), "△ ABC");
        assert_eq!(to_unicode("$\\angle A=90^{\\circ}$"), "∠ A=90°");
        assert_eq!(to_unicode("$\\underline{\\text{中线}}$"), "中线");
    }

    #[test]
    fn keeps_unknown() {
        assert_eq!(to_unicode("\\foo{bar}"), "foobar");
    }
}
