/// LaTeX 数学符号与宏命令映射表
/// 将 LaTeX 命令统一转换为标准 Unicode 数学字符

/// 查询 LaTeX 宏命令对应的 Unicode 字符
pub fn lookup_symbol(command: &str) -> Option<&'static str> {
    // 优先匹配希腊字母及变体
    if let Some(s) = lookup_greek(command) {
        return Some(s);
    }
    // 常见标准数学函数与算子名
    if let Some(s) = lookup_function_operator(command) {
        return Some(s);
    }
    // 微积分、分析与算子
    if let Some(s) = lookup_calculus(command) {
        return Some(s);
    }
    // 关系符与比较
    if let Some(s) = lookup_relations(command) {
        return Some(s);
    }
    // 集合论与数理逻辑
    if let Some(s) = lookup_sets_and_logic(command) {
        return Some(s);
    }
    // 二元运算符与特殊符号
    if let Some(s) = lookup_binary_ops(command) {
        return Some(s);
    }
    // 箭头与映射
    if let Some(s) = lookup_arrows(command) {
        return Some(s);
    }
    // 省略号与点
    if let Some(s) = lookup_dots(command) {
        return Some(s);
    }
    // 定界符与括号
    if let Some(s) = lookup_delimiters(command) {
        return Some(s);
    }
    // 黑板粗体数集、花体与常用字母表 (A-Z, a-z)
    if let Some(s) = lookup_blackboard_and_cal(command) {
        return Some(s);
    }
    // 希伯来字母、物理常数与特殊数学符号
    if let Some(s) = lookup_alphanumeric_and_special(command) {
        return Some(s);
    }
    None
}

/// 希腊字母及变体
fn lookup_greek(command: &str) -> Option<&'static str> {
    match command {
        // 小写希腊字母
        "alpha" => Some("α"),
        "beta" => Some("β"),
        "gamma" => Some("γ"),
        "delta" => Some("δ"),
        "epsilon" => Some("ϵ"),
        "varepsilon" => Some("ε"),
        "zeta" => Some("ζ"),
        "eta" => Some("η"),
        "theta" => Some("θ"),
        "vartheta" => Some("ϑ"),
        "iota" => Some("ι"),
        "kappa" => Some("κ"),
        "varkappa" => Some("ϰ"),
        "lambda" => Some("λ"),
        "mu" => Some("μ"),
        "nu" => Some("ν"),
        "xi" => Some("ξ"),
        "pi" => Some("π"),
        "varpi" => Some("ϖ"),
        "rho" => Some("ρ"),
        "varrho" => Some("ϱ"),
        "sigma" => Some("σ"),
        "varsigma" => Some("ς"),
        "tau" => Some("τ"),
        "upsilon" => Some("υ"),
        "phi" => Some("ϕ"),
        "varphi" => Some("φ"),
        "chi" => Some("χ"),
        "psi" => Some("ψ"),
        "omega" => Some("ω"),
        "digamma" => Some("ϝ"),

        // 大写希腊字母
        "Gamma" => Some("Γ"),
        "Delta" => Some("Δ"),
        "Theta" => Some("Θ"),
        "Lambda" => Some("Λ"),
        "Xi" => Some("Ξ"),
        "Pi" => Some("Π"),
        "Sigma" => Some("Σ"),
        "Upsilon" => Some("Υ"),
        "Phi" => Some("Φ"),
        "Psi" => Some("Ψ"),
        "Omega" => Some("Ω"),

        // 斜体大写希腊字母（常见别名）
        "varGamma" => Some("𝛤"),
        "varDelta" => Some("𝛥"),
        "varTheta" => Some("𝛩"),
        "varLambda" => Some("𝛬"),
        "varXi" => Some("𝛯"),
        "varPi" => Some("𝛱"),
        "varSigma" => Some("𝛴"),
        "varUpsilon" => Some("𝛶"),
        "varPhi" => Some("𝛷"),
        "varPsi" => Some("𝛹"),
        "varOmega" => Some("𝛺"),
        _ => None,
    }
}

/// 标准数学函数与算子名（保持正体）
fn lookup_function_operator(command: &str) -> Option<&'static str> {
    match command {
        // 三角与反三角
        "sin" => Some("sin"),
        "cos" => Some("cos"),
        "tan" => Some("tan"),
        "cot" => Some("cot"),
        "sec" => Some("sec"),
        "csc" => Some("csc"),
        "arcsin" => Some("arcsin"),
        "arccos" => Some("arccos"),
        "arctan" => Some("arctan"),
        "arccot" => Some("arccot"),
        "arcsec" => Some("arcsec"),
        "arccsc" => Some("arccsc"),

        // 双曲与反双曲
        "sinh" => Some("sinh"),
        "cosh" => Some("cosh"),
        "tanh" => Some("tanh"),
        "coth" => Some("coth"),
        "sech" => Some("sech"),
        "csch" => Some("csch"),
        "arsinh" | "arcsinh" => Some("arsinh"),
        "arcosh" | "arccosh" => Some("arcosh"),
        "artanh" | "arctanh" => Some("artanh"),

        // 对数与指数
        "ln" => Some("ln"),
        "log" => Some("log"),
        "lg" => Some("lg"),
        "exp" => Some("exp"),

        // 极值、极限与界限
        "lim" => Some("lim"),
        "liminf" => Some("lim inf"),
        "limsup" => Some("lim sup"),
        "min" => Some("min"),
        "max" => Some("max"),
        "sup" => Some("sup"),
        "inf" => Some("inf"),
        "arg" => Some("arg"),
        "argmax" => Some("arg max"),
        "argmin" => Some("arg min"),

        // 代数、几何、拓扑与概率统计
        "det" => Some("det"),
        "dim" => Some("dim"),
        "ker" => Some("ker"),
        "deg" => Some("deg"),
        "gcd" => Some("gcd"),
        "lcm" => Some("lcm"),
        "hom" => Some("hom"),
        "Pr" => Some("Pr"),
        "rank" => Some("rank"),
        "tr" | "trace" => Some("tr"),
        "diag" => Some("diag"),
        "mod" => Some("mod"),
        "sgn" => Some("sgn"),
        "sign" => Some("sign"),
        "span" => Some("span"),
        "var" | "Var" => Some("Var"),
        "cov" | "Cov" => Some("Cov"),
        "bias" | "Bias" => Some("Bias"),
        "se" | "SE" => Some("SE"),
        "mse" | "MSE" => Some("MSE"),
        "im" | "image" | "Image" => Some("im"),
        "codim" => Some("codim"),
        "supp" => Some("supp"),
        "vol" => Some("vol"),
        "dist" => Some("dist"),
        "card" => Some("card"),
        "proj" => Some("proj"),
        "grad" => Some("grad"),
        "curl" => Some("curl"),
        "divergence" | "Div" => Some("div"),
        "softmax" | "Softmax" => Some("softmax"),
        "sigmoid" | "Sigmoid" => Some("sigmoid"),
        "relu" | "ReLU" => Some("ReLU"),
        "gelu" | "GELU" => Some("GELU"),
        "loss" | "Loss" => Some("Loss"),
        "atan2" => Some("atan2"),
        "sinc" => Some("sinc"),
        _ => None,
    }
}

/// 微积分、分析与算子
fn lookup_calculus(command: &str) -> Option<&'static str> {
    match command {
        "partial" => Some("∂"),
        "nabla" => Some("∇"),
        "laplacian" => Some("Δ"),
        "hbar" => Some("ℏ"),
        "hslash" => Some("ℏ"),
        "ell" => Some("ℓ"),
        "wp" => Some("℘"),
        "int" => Some("∫"),
        "iint" => Some("∬"),
        "iiint" => Some("∭"),
        "iiiint" => Some("⨌"),
        "oint" => Some("∮"),
        "oiint" => Some("∯"),
        "oiiint" => Some("∰"),
        "intclockwise" => Some("∱"),
        "varointclockwise" => Some("∲"),
        "ointctrclockwise" => Some("∳"),
        "prime" => Some("′"),
        "dprime" => Some("″"),
        "trprime" => Some("‴"),
        "qprime" => Some("⁗"),
        "sum" => Some("∑"),
        "prod" => Some("∏"),
        "coprod" => Some("∐"),
        _ => None,
    }
}

/// 关系符与比较
fn lookup_relations(command: &str) -> Option<&'static str> {
    match command {
        // 大小与不等式
        "le" | "leq" => Some("≤"),
        "ge" | "geq" => Some("≥"),
        "ne" | "neq" => Some("≠"),
        "ll" => Some("≪"),
        "gg" => Some("≫"),
        "lll" | "llless" => Some("⋘"),
        "ggg" | "gggtr" => Some("⋙"),
        "lessapprox" => Some("⪅"),
        "gtrapprox" => Some("⪆"),
        "lesseqslant" => Some("⩽"),
        "gtreqslant" => Some("⩾"),
        "lneqq" => Some("≨"),
        "gneqq" => Some("≩"),
        "nle" | "nleq" => Some("≰"),
        "nge" | "ngeq" => Some("≱"),

        // 等价、相似与渐近
        "approx" => Some("≈"),
        "approxeq" => Some("≊"),
        "equiv" => Some("≡"),
        "sim" => Some("∼"),
        "simeq" => Some("≃"),
        "nsim" => Some("≁"),
        "cong" => Some("≅"),
        "ncong" => Some("≇"),
        "propto" | "varpropto" => Some("∝"),
        "doteq" => Some("≐"),
        "doteqdot" | "Doteq" => Some("≑"),
        "coloneqq" => Some("≔"),
        "eqqcolon" => Some("≕"),
        "asymp" => Some("≍"),
        "bowtie" => Some("⋈"),
        "smile" => Some("⌣"),
        "frown" => Some("⌢"),

        // 偏序与包含
        "prec" => Some("≺"),
        "succ" => Some("≻"),
        "preceq" => Some("⪯"),
        "succeq" => Some("⪰"),
        "precapprox" => Some("⪹"),
        "succapprox" => Some("⪺"),
        "precnapprox" => Some("⪹"),
        "succnapprox" => Some("⪺"),
        "nprec" => Some("⊀"),
        "nsucc" => Some("⊁"),
        "npreceq" => Some("⋠"),
        "nsucceq" => Some("⋡"),

        // 几何关系
        "parallel" => Some("∥"),
        "nparallel" => Some("∦"),
        "perp" => Some("⊥"),
        "pitchfork" => Some("⋔"),
        "mid" => Some("∣"),
        "nmid" => Some("∤"),
        _ => None,
    }
}

/// 集合论与数理逻辑
fn lookup_sets_and_logic(command: &str) -> Option<&'static str> {
    match command {
        // 属于与包含
        "in" => Some("∈"),
        "notin" => Some("∉"),
        "ni" | "owns" => Some("∋"),
        "subset" => Some("⊂"),
        "supset" => Some("⊃"),
        "subseteq" => Some("⊆"),
        "supseteq" => Some("⊇"),
        "subsetneq" => Some("⊊"),
        "supsetneq" => Some("⊋"),
        "subsetneqq" => Some("⫋"),
        "supsetneqq" => Some("⫌"),
        "sqsubset" => Some("⊏"),
        "sqsupset" => Some("⊐"),
        "sqsubseteq" => Some("⊑"),
        "sqsupseteq" => Some("⊒"),
        "sqsubsetneq" => Some("⋤"),
        "sqsupsetneq" => Some("⋥"),
        "nsubset" => Some("⊄"),
        "nsupset" => Some("⊅"),
        "nsubseteq" => Some("⊈"),
        "nsupseteq" => Some("⊉"),

        // 集合运算
        "cap" => Some("∩"),
        "cup" => Some("∪"),
        "bigcap" => Some("⋂"),
        "bigcup" => Some("⋃"),
        "uplus" => Some("⊎"),
        "biguplus" => Some("⨄"),
        "sqcap" => Some("⊓"),
        "bigsqcup" => Some("⊔"),
        "setminus" | "smallsetminus" => Some("∖"),
        "emptyset" | "varnothing" => Some("∅"),

        // 逻辑与量词
        "infty" => Some("∞"),
        "forall" => Some("∀"),
        "exists" => Some("∃"),
        "nexists" => Some("∄"),
        "land" | "wedge" => Some("∧"),
        "lor" | "vee" => Some("∨"),
        "bigwedge" => Some("⋀"),
        "bigvee" => Some("⋁"),
        "lnot" | "neg" => Some("¬"),
        "top" => Some("⊤"),
        "bot" => Some("⊥"),
        "vdash" => Some("⊢"),
        "dashv" => Some("⊣"),
        "models" => Some("⊨"),
        "vDash" => Some("⊨"),
        "Vdash" => Some("⊩"),
        "Vvdash" => Some("⊪"),
        "therefore" => Some("∴"),
        "because" => Some("∵"),
        _ => None,
    }
}

/// 二元运算符与特殊符号
fn lookup_binary_ops(command: &str) -> Option<&'static str> {
    match command {
        "pm" => Some("±"),
        "mp" => Some("∓"),
        "times" => Some("×"),
        "div" => Some("÷"),
        "cdot" => Some("·"),
        "centerdot" => Some("·"),
        "circ" => Some("∘"),
        "bullet" => Some("•"),
        "star" => Some("⋆"),
        "ast" => Some("∗"),
        "diamond" => Some("◇"),
        "Diamond" => Some("⋄"),
        "lozenge" => Some("◊"),
        "blacklozenge" => Some("✦"),
        "square" | "Box" => Some("□"),
        "blacksquare" => Some("■"),
        "triangle" => Some("△"),
        "triangledown" => Some("▽"),
        "vartriangle" => Some("∆"),
        "blacktriangle" => Some("▲"),
        "blacktriangledown" => Some("▼"),
        "triangleleft" => Some("◃"),
        "triangleright" => Some("▹"),
        "blacktriangleleft" => Some("◀"),
        "blacktriangleright" => Some("▶"),

        // 圈算子
        "oplus" => Some("⊕"),
        "ominus" => Some("⊖"),
        "otimes" => Some("⊗"),
        "oslash" => Some("⊘"),
        "odot" => Some("⊙"),
        "circledast" => Some("⊛"),
        "circledcirc" => Some("⊚"),
        "circleddash" => Some("⊝"),
        "bigoplus" => Some("⨁"),
        "bigotimes" => Some("⨂"),
        "bigodot" => Some("⨀"),
        "boxplus" => Some("⊞"),
        "boxminus" => Some("⊟"),
        "boxtimes" => Some("⊠"),
        "boxdot" => Some("⊡"),

        // 其他二元符号
        "amalg" => Some("⨿"),
        "wr" => Some("≀"),
        "dagger" => Some("†"),
        "ddagger" => Some("‡"),
        "angle" => Some("∠"),
        "measuredangle" => Some("∡"),
        "sphericalangle" => Some("∢"),
        "diagup" => Some("╱"),
        "diagdown" => Some("╲"),
        "flat" => Some("♭"),
        "natural" => Some("♮"),
        "sharp" => Some("♯"),
        "heartsuit" => Some("♡"),
        "spadesuit" => Some("♠"),
        "diamondsuit" => Some("♢"),
        "clubsuit" => Some("♣"),
        _ => None,
    }
}

/// 箭头与映射
fn lookup_arrows(command: &str) -> Option<&'static str> {
    match command {
        // 短单线箭头
        "to" | "rightarrow" => Some("→"),
        "leftarrow" | "gets" => Some("←"),
        "uparrow" => Some("↑"),
        "downarrow" => Some("↓"),
        "leftrightarrow" => Some("↔"),
        "updownarrow" => Some("↕"),
        "nearrow" => Some("↗"),
        "searrow" => Some("↘"),
        "swarrow" => Some("↙"),
        "nwarrow" => Some("↖"),

        // 双线逻辑蕴含箭头
        "Rightarrow" | "implies" => Some("⇒"),
        "Leftarrow" => Some("⇐"),
        "Leftrightarrow" | "iff" => Some("⇔"),
        "Uparrow" => Some("⇑"),
        "Downarrow" => Some("⇓"),
        "Updownarrow" => Some("⇕"),

        // 长箭头
        "longrightarrow" => Some("⟶"),
        "longleftarrow" => Some("⟵"),
        "longleftrightarrow" => Some("⟷"),
        "Longrightarrow" => Some("⟹"),
        "Longleftarrow" => Some("⟸"),
        "Longleftrightarrow" => Some("⟺"),

        // 映射箭头
        "mapsto" => Some("↦"),
        "longmapsto" => Some("⟼"),

        // 弯钩与特殊末端箭头
        "hookleftarrow" => Some("↩"),
        "hookrightarrow" => Some("↪"),
        "looparrowleft" => Some("↫"),
        "looparrowright" => Some("↬"),
        "twoheadrightarrow" => Some("↠"),
        "twoheadleftarrow" => Some("↞"),
        "rightarrowtail" => Some("↣"),
        "leftarrowtail" => Some("↢"),

        // 半鱼叉半箭头（Harpoons）
        "rightharpoonup" => Some("⇀"),
        "rightharpoondown" => Some("⇁"),
        "leftharpoonup" => Some("↼"),
        "leftharpoondown" => Some("⇃"),
        "upharpoonleft" => Some("↿"),
        "upharpoonright" => Some("↾"),
        "downharpoonleft" => Some("⇃"),
        "downharpoonright" => Some("⇂"),
        "rightleftharpoons" => Some("⇌"),
        "leftrightharpoons" => Some("⇋"),

        // 否定箭头
        "nrightarrow" => Some("↛"),
        "nleftarrow" => Some("↚"),
        "nRightarrow" => Some("⇏"),
        "nLeftarrow" => Some("⇍"),
        "nLeftrightarrow" => Some("⇎"),

        // 曲线与环形箭头
        "curvearrowleft" => Some("↶"),
        "curvearrowright" => Some("↷"),
        "circlearrowleft" => Some("↺"),
        "circlearrowright" => Some("↻"),
        "leadsto" => Some("↝"),
        "leftrightsquigarrow" => Some("↭"),
        _ => None,
    }
}

/// 省略号与点
fn lookup_dots(command: &str) -> Option<&'static str> {
    match command {
        "dots" | "ldots" => Some("…"),
        "cdots" => Some("⋯"),
        "vdots" => Some("⋮"),
        "ddots" => Some("⋱"),
        "iddots" | "adots" => Some("⋰"),
        _ => None,
    }
}

/// 定界符与括号
fn lookup_delimiters(command: &str) -> Option<&'static str> {
    match command {
        "langle" => Some("⟨"),
        "rangle" => Some("⟩"),
        "lceil" => Some("⌈"),
        "rceil" => Some("⌉"),
        "lfloor" => Some("⌊"),
        "rfloor" => Some("⌋"),
        "vert" => Some("|"),
        "Vert" => Some("‖"),
        "lgroup" => Some("⦗"),
        "rgroup" => Some("⦘"),
        "llbracket" => Some("⟦"),
        "rrbracket" => Some("⟧"),
        _ => None,
    }
}

/// 黑板粗体数集、花体与全套字母表 (A-Z, a-z)
fn lookup_blackboard_and_cal(command: &str) -> Option<&'static str> {
    match command {
        // 黑板粗体大写字母 (A-Z)
        "mathbbA" => Some("𝔸"),
        "mathbbB" => Some("𝔹"),
        "mathbbC" | "C" => Some("ℂ"),
        "mathbbD" => Some("𝔻"),
        "mathbbE" => Some("𝔼"),
        "mathbbF" => Some("𝔽"),
        "mathbbG" => Some("𝔾"),
        "mathbbH" => Some("ℍ"),
        "mathbbI" => Some("𝕀"),
        "mathbbJ" => Some("𝕁"),
        "mathbbK" => Some("𝕂"),
        "mathbbL" => Some("𝕃"),
        "mathbbM" => Some("𝕄"),
        "mathbbN" | "N" => Some("ℕ"),
        "mathbbO" => Some("𝕆"),
        "mathbbP" => Some("ℙ"),
        "mathbbQ" | "Q" => Some("ℚ"),
        "mathbbR" | "R" => Some("ℝ"),
        "mathbbS" => Some("𝕊"),
        "mathbbT" => Some("𝕋"),
        "mathbbU" => Some("𝕌"),
        "mathbbV" => Some("𝕍"),
        "mathbbW" => Some("𝕎"),
        "mathbbX" => Some("𝕏"),
        "mathbbY" => Some("𝕐"),
        "mathbbZ" | "Z" => Some("ℤ"),
        "mathbbOne" | "mathbb1" => Some("𝟙"),
        "mathbbZero" | "mathbb0" => Some("𝟘"),

        // 手写花体大写字母 (mathcal A-Z)
        "mathcalA" => Some("𝒜"),
        "mathcalB" => Some("ℬ"),
        "mathcalC" => Some("𝒞"),
        "mathcalD" => Some("𝒟"),
        "mathcalE" => Some("ℰ"),
        "mathcalF" => Some("ℱ"),
        "mathcalG" => Some("𝒢"),
        "mathcalH" => Some("ℋ"),
        "mathcalI" => Some("ℐ"),
        "mathcalJ" => Some("𝒥"),
        "mathcalK" => Some("𝒦"),
        "mathcalL" => Some("ℒ"),
        "mathcalM" => Some("ℳ"),
        "mathcalN" => Some("𝒩"),
        "mathcalO" => Some("𝒪"),
        "mathcalP" => Some("𝒫"),
        "mathcalQ" => Some("𝒬"),
        "mathcalR" => Some("ℛ"),
        "mathcalS" => Some("𝒮"),
        "mathcalT" => Some("𝒯"),
        "mathcalU" => Some("𝒰"),
        "mathcalV" => Some("𝒱"),
        "mathcalW" => Some("𝒲"),
        "mathcalX" => Some("𝒳"),
        "mathcalY" => Some("𝒴"),
        "mathcalZ" => Some("𝒵"),

        // 德文哥特体/分形体字母 (mathfrak A-Z, a-z)
        "mathfrakA" | "frakA" => Some("𝔄"),
        "mathfrakB" | "frakB" => Some("𝔅"),
        "mathfrakC" | "frakC" => Some("ℭ"),
        "mathfrakD" | "frakD" => Some("𝔇"),
        "mathfrakE" | "frakE" => Some("𝔈"),
        "mathfrakF" | "frakF" => Some("𝔉"),
        "mathfrakG" | "frakG" => Some("𝔊"),
        "mathfrakH" | "frakH" => Some("ℌ"),
        "mathfrakI" | "frakI" => Some("ℑ"),
        "mathfrakJ" | "frakJ" => Some("𝔍"),
        "mathfrakK" | "frakK" => Some("𝔎"),
        "mathfrakL" | "frakL" => Some("𝔏"),
        "mathfrakM" | "frakM" => Some("𝔐"),
        "mathfrakN" | "frakN" => Some("𝔑"),
        "mathfrakO" | "frakO" => Some("𝔒"),
        "mathfrakP" | "frakP" => Some("𝔓"),
        "mathfrakQ" | "frakQ" => Some("𝔔"),
        "mathfrakR" | "frakR" => Some("ℜ"),
        "mathfrakS" | "frakS" => Some("𝔖"),
        "mathfrakT" | "frakT" => Some("𝔗"),
        "mathfrakU" | "frakU" => Some("𝔘"),
        "mathfrakV" | "frakV" => Some("𝔙"),
        "mathfrakW" | "frakW" => Some("𝔚"),
        "mathfrakX" | "frakX" => Some("𝔛"),
        "mathfrakY" | "frakY" => Some("𝔜"),
        "mathfrakZ" | "frakZ" => Some("ℨ"),
        "mathfraka" | "fraka" => Some("𝔞"),
        "mathfrakb" | "frakb" => Some("𝔟"),
        "mathfrakc" | "frakc" => Some("𝔠"),
        "mathfrakd" | "frakd" => Some("𝔡"),
        "mathfrake" | "frake" => Some("𝔢"),
        "mathfrakf" | "frakf" => Some("𝔣"),
        "mathfrakg" | "frakg" => Some("𝔤"),
        "mathfrakh" | "frakh" => Some("𝔥"),
        "mathfraki" | "fraki" => Some("𝔦"),
        "mathfrakj" | "frakj" => Some("𝔧"),
        "mathfrakk" | "frakk" => Some("𝔨"),
        "mathfrakl" | "frakl" => Some("𝔩"),
        "mathfrakm" | "frakm" => Some("𝔪"),
        "mathfrakn" | "frakn" => Some("𝔫"),
        "mathfrako" | "frako" => Some("𝔬"),
        "mathfrakp" | "frakp" => Some("𝔭"),
        "mathfrakq" | "frakq" => Some("𝔮"),
        "mathfrakr" | "frakr" => Some("𝔯"),
        "mathfraks" | "fraks" => Some("𝔰"),
        "mathfrakt" | "frakt" => Some("𝔱"),
        "mathfraku" | "fraku" => Some("𝔲"),
        "mathfrakv" | "frakv" => Some("𝔳"),
        "mathfrakw" | "frakw" => Some("𝔴"),
        "mathfrakx" | "frakx" => Some("𝔵"),
        "mathfraky" | "fraky" => Some("𝔶"),
        "mathfrakz" | "frakz" => Some("𝔷"),
        "mathfrakgl" => Some("𝔤𝔩"),
        "mathfrakso" => Some("𝔰𝔬"),
        "mathfraksp" => Some("𝔰𝔭"),

        // Script 脚本花体大写字母 (mathscr A-Z)
        "mathscrA" => Some("𝒜"),
        "mathscrB" => Some("ℬ"),
        "mathscrC" => Some("𝒞"),
        "mathscrD" => Some("𝒟"),
        "mathscrE" => Some("ℰ"),
        "mathscrF" => Some("ℱ"),
        "mathscrG" => Some("𝒢"),
        "mathscrH" => Some("ℋ"),
        "mathscrI" => Some("ℐ"),
        "mathscrJ" => Some("𝒥"),
        "mathscrK" => Some("𝒦"),
        "mathscrL" => Some("ℒ"),
        "mathscrM" => Some("ℳ"),
        "mathscrN" => Some("𝒩"),
        "mathscrO" => Some("𝒪"),
        "mathscrP" => Some("𝒫"),
        "mathscrQ" => Some("𝒬"),
        "mathscrR" => Some("ℛ"),
        "mathscrS" => Some("𝒮"),
        "mathscrT" => Some("𝒯"),
        "mathscrU" => Some("𝒰"),
        "mathscrV" => Some("𝒱"),
        "mathscrW" => Some("𝒲"),
        "mathscrX" => Some("𝒳"),
        "mathscrY" => Some("𝒴"),
        "mathscrZ" => Some("𝒵"),
        _ => None,
    }
}

/// 字母、希伯来字母与特殊数学常数/字符
fn lookup_alphanumeric_and_special(command: &str) -> Option<&'static str> {
    match command {
        "aleph" => Some("ℵ"),
        "beth" => Some("ℶ"),
        "gimel" => Some("ℷ"),
        "daleth" => Some("ℸ"),
        "Re" => Some("ℜ"),
        "Im" => Some("ℑ"),
        "S" => Some("§"),
        "P" => Some("¶"),
        "checkmark" => Some("✓"),
        "yen" => Some("¥"),
        "euro" => Some("€"),
        "pounds" | "sterling" => Some("£"),
        "copyright" => Some("©"),
        "dag" => Some("†"),
        "ddag" => Some("‡"),
        "degree" | "deg" => Some("°"),
        "celsius" => Some("℃"),
        "fahrenheit" => Some("℉"),
        "hbar" => Some("ℏ"),
        "mho" => Some("℧"),
        _ => None,
    }
}
