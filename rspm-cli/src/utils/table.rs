use table::Table;
use colored::Colorize;

/// 创建一个新的表格
pub fn create_table(headers: Vec<&str>) -> TableBuilder {
    TableBuilder {
        headers: headers.iter().map(|s| s.to_string()).collect(),
        rows: Vec::new(),
    }
}

/// 表格构建器
pub struct TableBuilder {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl TableBuilder {
    /// 添加一行数据
    pub fn row(mut self, row: Vec<String>) -> Self {
        self.rows.push(row);
        self
    }

    /// 添加多行数据
    pub fn rows(mut self, rows: Vec<Vec<String>>) -> Self {
        self.rows.extend(rows);
        self
    }

    /// 渲染并打印表格
    pub fn print(self) {
        let mut table = Table::new();
        // 使用简单的 ASCII 制表符
        table.set_tabs("─ ─────────");
        table.set_header(&self.headers);
        
        for row in &self.rows {
            table.add_row(row);
        }
        
        println!("{}", table.render());
    }
}

/// 打印空表格（当没有数据时显示提示信息）
pub fn print_empty_table(headers: Vec<&str>, message: &str) {
    let mut table = Table::new();
    // 使用简单的 ASCII 制表符
    table.set_tabs("─ ─────────");
    table.set_header(&headers.iter().map(|s| s.to_string()).collect());
    
    // 添加一个空行
    let empty_row = vec![message.to_string(); headers.len()];
    table.add_row(&empty_row);
    
    println!("{}", table.render());
}

/// 根据状态字符串获取颜色
pub fn get_status_color(status: &str) -> String {
    match status {
        "running" => status.green().to_string(),
        "starting" => status.yellow().to_string(),
        "stopping" => status.yellow().to_string(),
        "stopped" => status.cyan().to_string(),
        "errored" => status.red().to_string(),
        _ => status.to_string(),
    }
}
