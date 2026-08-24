pub mod element;
pub mod engine;
pub mod font;
pub mod layout_tree;
pub mod symbols;

pub use element::MathElement;
pub use engine::MathEngine;
pub use font::{KATEX_MAIN_FONT, KATEX_MATH_FONT};
pub use layout_tree::{LayoutResult, MathNode};
pub use symbols::lookup_symbol;

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    #[test]
    fn test_engine_empty_input() {
        let res = MathEngine::layout("", px(14.0), false).unwrap();
        assert_eq!(res.width, px(0.0));
        assert_eq!(res.height, px(0.0));
        assert!(res.nodes.is_empty());
    }

    #[test]
    fn test_engine_fraction_layout() {
        let res = MathEngine::layout(r"\frac{a}{b}", px(14.0), true).unwrap();
        assert!(res.width > px(0.0));
        assert!(res.height > px(0.0));
        // 应包含 1 条分数线和 2 个字符节点
        assert_eq!(res.nodes.len(), 3);
    }

    #[test]
    fn test_engine_greek_and_sub_sup() {
        let res = MathEngine::layout(r"\alpha^2 + \beta_i = \gamma", px(14.0), false).unwrap();
        assert!(res.width > px(0.0));
        assert!(res.height > px(0.0));
        assert!(!res.nodes.is_empty());
    }

    #[test]
    fn test_engine_complex_formula() {
        let res = MathEngine::layout(r"E = mc^2 + \frac{\hbar \omega}{2}", px(14.0), true).unwrap();
        assert!(res.width > px(0.0));
        assert!(res.height > px(0.0));
    }

    #[test]
    fn test_engine_sqrt() {
        let res = MathEngine::layout(r"\sqrt{x^2 + y^2}", px(14.0), false).unwrap();
        assert!(res.width > px(0.0));
        assert!(res.height > px(0.0));
        // 应包含根号横线 Rule
        assert!(res.nodes.iter().any(|n| matches!(n, MathNode::Rule { .. })));

        let res_indexed = MathEngine::layout(r"\sqrt[3]{8}", px(14.0), true).unwrap();
        assert!(res_indexed.width > px(0.0));
    }

    #[test]
    fn test_engine_accents() {
        let res_hat = MathEngine::layout(
            r"\hat{y} = \bar{x} + \tilde{\theta} + \vec{v}",
            px(14.0),
            false,
        )
        .unwrap();
        assert!(res_hat.width > px(0.0));
        assert!(res_hat.height > px(0.0));

        let res_dot = MathEngine::layout(r"\dot{x} + \ddot{y} = 0", px(14.0), true).unwrap();
        assert!(res_dot.width > px(0.0));
    }

    #[test]
    fn test_engine_simultaneous_sub_sup() {
        // 测试同时存在下标与上标：x_i^2
        let res1 = MathEngine::layout(r"x_i^2 + y^2_j = z_{i,j}^{n+1}", px(14.0), false).unwrap();
        assert!(res1.width > px(0.0));
        assert!(res1.height > px(0.0));

        // 测试求和上下标同轴：\sum_{i=1}^n
        let res2 = MathEngine::layout(r"\sum_{i=1}^n x_i", px(14.0), true).unwrap();
        assert!(res2.width > px(0.0));
    }

    #[test]
    fn test_engine_pde_and_calculus() {
        // 偏微分方程（如热传导 / 波动方程）：\frac{\partial u}{\partial t} = \alpha \frac{\partial^2 u}{\partial x^2}
        let res_pde = MathEngine::layout(
            r"\frac{\partial u}{\partial t} = \alpha \frac{\partial^2 u}{\partial x^2}",
            px(14.0),
            true,
        )
        .unwrap();
        assert!(res_pde.width > px(0.0));
        assert!(res_pde.height > px(0.0));

        // 常微分方程与初值（如带导数和积分项）：y'' + 2\zeta \omega_n y' + \omega_n^2 y = \int_0^t f(\tau) d\tau
        let res_ode = MathEngine::layout(
            r"y'' + 2\zeta \omega_n y' + \omega_n^2 y = \int_0^t f(\tau) d\tau",
            px(14.0),
            true,
        )
        .unwrap();
        assert!(res_ode.width > px(0.0));

        // 麦克斯韦方程组与纳维-斯托克斯偏微分（梯度、散度、旋度、多重积分）：\oiint \mathbf{B} \cdot d\mathbf{S} = 0, \nabla \cdot \vec{u} = 0
        let res_maxwell = MathEngine::layout(
            r"\oiint \vec{B} \cdot d\vec{S} = 0 + \nabla \times \vec{E} = -\frac{\partial \vec{B}}{\partial t}",
            px(14.0),
            true,
        )
        .unwrap();
        assert!(res_maxwell.width > px(0.0));
    }

    #[test]
    fn test_new_symbols_and_operators() {
        // 测试三角函数、极限、概率、集合与逻辑符号
        let res1 = MathEngine::layout(
            r"\lim_{x \to 0} \frac{\sin x}{x} = 1 \implies \ln(e) = 1",
            px(14.0),
            true,
        )
        .unwrap();
        assert!(res1.width > px(0.0));

        let res2 = MathEngine::layout(
            r"\forall x \in \mathbb{R}, \exists y : x \le y \land y \neq \infty",
            px(14.0),
            false,
        )
        .unwrap();
        assert!(res2.width > px(0.0));

        let res3 = MathEngine::layout(
            r"\langle u, v \rangle \le \|u\| \cdot \|v\| \quad \dots \cdots",
            px(14.0),
            false,
        )
        .unwrap();
        assert!(res3.width > px(0.0));

        // 测试新增的几何、统计、半箭头与矩阵点号
        let res4 = MathEngine::layout(
            r"\Var(X) = \Cov(X, X) \iff A \otimes B \boxtimes C \rightleftharpoons D \iddots",
            px(14.0),
            true,
        )
        .unwrap();
        assert!(res4.width > px(0.0));

        // 测试 \mathbb{R}, \mathcal{L}, \text{...}
        let res5 = MathEngine::layout(
            r"\mathcal{L}(\theta) = \mathbb{E}_{x \sim p}[ \text{loss}(x; \theta) ] + \lambda \|\theta\|^2",
            px(14.0),
            true,
        )
        .unwrap();
        assert!(res5.width > px(0.0));
        assert!(res5.height > px(0.0));

        // 测试转义大括号 \{ \}, 细空格 \,, \left( \right), 全套花体与黑板粗体
        let res6 = MathEngine::layout(
            r"S = \{ x \in \mathbb{A} \mid \left( x + 1 \right) \in \mathcal{Z} \}",
            px(14.0),
            false,
        )
        .unwrap();
        assert!(res6.width > px(0.0));

        // 测试多行矩阵 \begin{pmatrix} a & b \\ c & d \end{pmatrix}
        let res7 = MathEngine::layout(
            r"A = \begin{pmatrix} a_{11} & a_{12} \\ a_{21} & a_{22} \end{pmatrix} \begin{bmatrix} x \\ y \end{bmatrix}",
            px(14.0),
            true,
        )
        .unwrap();
        assert!(res7.width > px(0.0));
        assert!(res7.height > px(0.0));

        // 测试分段函数 \begin{cases} ... \end{cases} 与多行对齐 \begin{aligned} ... \end{aligned}
        let res8 = MathEngine::layout(
            r"f(x) = \begin{cases} \frac{x^2 - 1}{x - 1}, & x \neq 1 \\ 2, & x = 1 \end{cases}",
            px(14.0),
            true,
        )
        .unwrap();
        assert!(res8.width > px(0.0));
        assert!(res8.height > px(0.0));

        // 测试 \left( ... \right) 针对大分式自动拉伸定界符
        let res9 = MathEngine::layout(
            r"\left( \frac{\partial^2 u}{\partial x^2} + \frac{\partial^2 u}{\partial y^2} \right) \cdot \left[ 1 + \left( \frac{a}{b} \right)^2 \right]",
            px(14.0),
            true,
        )
        .unwrap();
        assert!(res9.width > px(0.0));
        assert!(res9.height > px(0.0));
    }
}
