use colored::Colorize;
use std::{collections::HashMap, usize};

use colored::CustomColor;

/// 计算字符串的显示宽度（中文字符算 1 个宽度）
fn str_width(s: &str) -> usize {
    s.chars().count()
}

#[derive(Debug, Clone, Copy, Default)]
pub enum CellAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug)]
pub struct Table {
    // 制表符：─│┌┬┐├┼┤└┴┘
    tabs: Vec<char>,
    // 表头数据
    header: Vec<String>,
    // 行数据
    rows: Vec<Vec<String>>,
    // 单元格宽度
    cell_width: Vec<usize>,
    // 设置单元格内容颜色
    cell_colors: HashMap<String, CustomColor>,
    row_colors: HashMap<usize, CustomColor>,
    col_colors: HashMap<usize, CustomColor>,
    // 单元格对齐方式
    cell_aligns: HashMap<String, CellAlign>,
    col_aligns: HashMap<usize, CellAlign>,
    // 合并单元格 (row, col) -> (row_span, col_span)
    merged_cells: HashMap<(usize, usize), (usize, usize)>,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    pub fn new() -> Self {
        // 默认制表符：─│┌┬┐├┼┤└┴┘
        let tabs = "─│┌┬┐├┼┤└┴┘".chars().collect();

        Table {
            tabs,
            header: Vec::new(),
            rows: Vec::new(),
            cell_width: Vec::new(),
            cell_colors: HashMap::new(),
            row_colors: HashMap::new(),
            col_colors: HashMap::new(),
            cell_aligns: HashMap::new(),
            col_aligns: HashMap::new(),
            merged_cells: HashMap::new(),
        }
    }

    // 设置制表符
    pub fn set_tabs(&mut self, s: &str) {
        let tabs: Vec<char> = s.chars().collect();
        if tabs.len() >= self.tabs.len() {
            self.tabs = tabs;
        }
    }

    // 设置某一单元格内容颜色
    pub fn set_cell_colors(&mut self, row: usize, col: usize, color: CustomColor) {
        let key = format!("{}_{}", row, col);
        self.cell_colors.insert(key, color);
    }

    // 设置某一行内容颜色
    pub fn set_row_colors(&mut self, row_idx: usize, color: CustomColor) {
        self.row_colors.insert(row_idx, color);
    }

    // 设置某一列内容颜色
    pub fn set_col_colors(&mut self, col_idx: usize, color: CustomColor) {
        self.col_colors.insert(col_idx, color);
    }

    // 设置单元格对齐方式
    pub fn set_cell_align(&mut self, row: usize, col: usize, align: CellAlign) {
        let key = format!("{}_{}", row, col);
        self.cell_aligns.insert(key, align);
    }

    // 设置列对齐方式
    pub fn set_col_align(&mut self, col_idx: usize, align: CellAlign) {
        self.col_aligns.insert(col_idx, align);
    }

    // 合并单元格
    pub fn merge_cells(&mut self, row: usize, col: usize, row_span: usize, col_span: usize) {
        self.merged_cells.insert((row, col), (row_span, col_span));
    }

    // 设置表头
    pub fn set_header(&mut self, v: &Vec<String>) {
        self.cell_width = Vec::new();
        for item in v {
            let width = str_width(item);
            self.cell_width.push(width);
        }
        self.header = v.to_vec();
    }

    // 添加数据行
    pub fn add_row(&mut self, row: &Vec<String>) {
        for (index, value) in row.iter().enumerate() {
            let width = str_width(value);
            if self.cell_width.len() <= index {
                self.cell_width.push(width);
            } else if self.cell_width[index] < width {
                self.cell_width[index] = width;
            }
        }
        // 如果数据行元素少于表头列数，用表头宽度填充剩余列
        while self.cell_width.len() < self.header.len() {
            let idx = self.cell_width.len();
            self.cell_width.push(str_width(&self.header[idx]));
        }
        self.rows.push(row.to_vec());
    }

    // 设置数据
    pub fn set_rows(&mut self, rows: &Vec<Vec<String>>) {
        for row in rows {
            for (index, value) in row.iter().enumerate() {
                let width = str_width(value);
                if self.cell_width.len() <= index {
                    self.cell_width.push(width);
                } else if self.cell_width[index] < width {
                    self.cell_width[index] = width;
                }
            }
        }
        self.rows = rows.to_vec();
    }

    // 获取单元格的对齐方式
    fn get_align(&self, row: usize, col: usize) -> CellAlign {
        let key = format!("{}_{}", row, col);
        if let Some(&align) = self.cell_aligns.get(&key) {
            return align;
        }
        if let Some(&align) = self.col_aligns.get(&col) {
            return align;
        }
        CellAlign::Left
    }

    // 格式化单元格内容
    fn format_cell(&self, content: &str, width: usize, align: CellAlign) -> String {
        let content_width = str_width(content);
        let padding = width.saturating_sub(content_width);

        match align {
            CellAlign::Left => {
                format!("{}{}", content, " ".repeat(padding))
            }
            CellAlign::Center => {
                let left_pad = padding / 2;
                let right_pad = padding - left_pad;
                format!(
                    "{}{}{}",
                    " ".repeat(left_pad),
                    content,
                    " ".repeat(right_pad)
                )
            }
            CellAlign::Right => {
                format!("{}{}", " ".repeat(padding), content)
            }
        }
    }

    // 检查单元格是否被合并跳过
    fn is_merged_skip(&self, row: usize, col: usize) -> bool {
        // 检查是否在其他合并单元格的范围内
        for (&(start_row, start_col), &(row_span, col_span)) in &self.merged_cells {
            if row >= start_row
                && row < start_row + row_span
                && col >= start_col
                && col < start_col + col_span
                && (row != start_row || col != start_col)
            {
                return true;
            }
        }
        false
    }

    // 获取合并单元格的跨度
    fn get_merge_span(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        self.merged_cells.get(&(row, col)).copied()
    }

    // 计算合并单元格的实际宽度（不包括左右边框制表符）
    fn get_merged_width(&self, col: usize, col_span: usize) -> usize {
        let mut total_width = 0;
        for i in 0..col_span {
            if col + i < self.cell_width.len() {
                // 每列宽度包括：1 空格 + 内容宽度 + 1 空格 = 内容宽度 + 2
                total_width += self.cell_width[col + i] + 2;
            }
        }
        // 减去内部被合并的制表符数量 (col_span - 1)
        total_width -= col_span - 1;
        total_width
    }

    pub fn render(&self) -> String {
        if self.cell_width.is_empty() {
            return String::new();
        }

        let mut result = String::new();

        // 构建顶部边框
        let mut top_line = String::new();
        top_line.push(self.tabs[2]); // ┌
        for (i, &w) in self.cell_width.iter().enumerate() {
            top_line.push_str(&"─".repeat(w + 2));
            if i < self.cell_width.len() - 1 {
                top_line.push(self.tabs[3]); // ┬
            }
        }
        top_line.push(self.tabs[4]); // ┐
        top_line.push_str("\r\n");

        // 构建表头
        let mut header_line = String::new();
        for (col_idx, &w) in self.cell_width.iter().enumerate() {
            header_line.push(self.tabs[1]); // │
            if col_idx < self.header.len() {
                let content = self.format_cell(&self.header[col_idx], w, CellAlign::Left);
                header_line.push(' ');
                header_line.push_str(&content);
                header_line.push(' ');
            } else {
                header_line.push(' ');
                header_line.push_str(&" ".repeat(w));
                header_line.push(' ');
            }
        }
        header_line.push(self.tabs[1]); // │
        header_line.push_str("\r\n");

        // 构建中间分隔线
        let mut mid_line = String::new();
        mid_line.push(self.tabs[5]); // ├
        for (i, &w) in self.cell_width.iter().enumerate() {
            mid_line.push_str(&"─".repeat(w + 2));
            if i < self.cell_width.len() - 1 {
                mid_line.push(self.tabs[6]); // ┼
            }
        }
        mid_line.push(self.tabs[7]); // ┤
        mid_line.push_str("\r\n");

        // 构建底部边框
        let mut foot_line = String::new();
        foot_line.push(self.tabs[8]); // └
        for (i, &w) in self.cell_width.iter().enumerate() {
            foot_line.push_str(&"─".repeat(w + 2));
            if i < self.cell_width.len() - 1 {
                foot_line.push(self.tabs[9]); // ┴
            }
        }
        foot_line.push(self.tabs[10]); // ┘

        // 组装表格
        result.push_str(&top_line);
        result.push_str(&header_line);

        if !self.rows.is_empty() {
            result.push_str(&mid_line);

            // 渲染数据行
            for (row_idx, row) in self.rows.iter().enumerate() {
                let mut row_line = String::new();
                let mut col_idx = 0;

                while col_idx < self.cell_width.len() {
                    // 检查是否跳过（被合并）
                    if self.is_merged_skip(row_idx, col_idx) {
                        col_idx += 1;
                        continue;
                    }

                    row_line.push(self.tabs[1]); // │

                    // 检查是否有合并单元格
                    if let Some((_row_span, col_span)) = self.get_merge_span(row_idx, col_idx) {
                        // 合并单元格
                        let total_width = self.get_merged_width(col_idx, col_span);
                        let content = if !row.is_empty() && col_idx < row.len() {
                            &row[col_idx]
                        } else {
                            ""
                        };
                        // 减去左右空格各 1 个
                        let content_width = total_width - 2;
                        let formatted = self.format_cell(content, content_width, CellAlign::Center);
                        row_line.push(' ');
                        row_line.push_str(&formatted);
                        row_line.push(' '); // 右侧空格
                        col_idx += col_span;
                    } else {
                        // 普通单元格
                        let w = self.cell_width[col_idx];
                        let content = if col_idx < row.len() {
                            &row[col_idx]
                        } else {
                            ""
                        };

                        // 获取颜色
                        let key = format!("{}_{}", row_idx, col_idx);
                        let cell_color = self.cell_colors.get(&key);
                        let col_color = self.col_colors.get(&col_idx);
                        let row_color = self.row_colors.get(&row_idx);

                        let mut styled_content = content.to_string();
                        if let Some(&color) = cell_color {
                            styled_content = styled_content.custom_color(color).to_string();
                        } else if let Some(&color) = col_color {
                            styled_content = styled_content.custom_color(color).to_string();
                        } else if let Some(&color) = row_color {
                            styled_content = styled_content.custom_color(color).to_string();
                        }

                        let formatted =
                            self.format_cell(&styled_content, w, self.get_align(row_idx, col_idx));
                        row_line.push(' ');
                        row_line.push_str(&formatted);
                        row_line.push(' ');
                        col_idx += 1;
                    }
                }
                row_line.push(self.tabs[1]); // │
                row_line.push_str("\r\n");

                result.push_str(&row_line);

                // 添加行之间的分隔线（如果不是最后一行）
                if row_idx < self.rows.len() - 1 {
                    result.push_str(&mid_line);
                }
            }
        }

        result.push_str(&foot_line);
        result
    }
}
