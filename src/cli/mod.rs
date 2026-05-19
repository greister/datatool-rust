use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};

use crate::config::Config;

#[derive(Parser)]
#[command(name = "datatool", version, about = "通达信深沪行情数据处理工具 - Rust版")]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Day {
        #[command(subcommand)]
        action: DayAction,
    },
    Tick {
        #[command(subcommand)]
        action: TickAction,
    },
    Min {
        #[command(subcommand)]
        action: MinAction,
    },
}

#[derive(Subcommand)]
pub enum DayAction {
    Create {
        start_date: String,
        #[arg(default_value = None)]
        end_date: Option<String>,
    },
    Del {
        start_date: String,
        #[arg(default_value = None)]
        end_date: Option<String>,
    },
    Check {
        #[arg(default_value = "all")]
        scope: String,
    },
}

#[derive(Subcommand)]
pub enum TickAction {
    Create {
        start_date: String,
        #[arg(default_value = None)]
        end_date: Option<String>,
    },
    Del {
        start_date: String,
        #[arg(default_value = None)]
        end_date: Option<String>,
    },
    Check {
        #[arg(default_value = "all")]
        scope: String,
    },
}

#[derive(Subcommand)]
pub enum MinAction {
    Create {
        #[arg(default_value = None)]
        start_or_all: Option<String>,
        #[arg(default_value = None)]
        end_date: Option<String>,
    },
    Del {
        start_date: String,
        #[arg(default_value = None)]
        end_date: Option<String>,
    },
}

impl Args {
    pub fn run(self) -> Result<()> {
        let config = Config::load("datatool.ini")?;
        let vipdoc = config.vipdoc_path()?;

        match self.command {
            Command::Day { action } => match action {
                DayAction::Create {
                    start_date,
                    end_date,
                } => {
                    let end = end_date.unwrap_or_else(|| start_date.clone());
                    println!("转档日线数据: {} - {}", start_date, end);
                    crate::day::create_day(&vipdoc, &start_date, &end)?;
                }
                DayAction::Del {
                    start_date,
                    end_date,
                } => {
                    let end = end_date.unwrap_or_else(|| start_date.clone());
                    println!("删除日线数据: {} - {}", start_date, end);
                    crate::day::del_day(&vipdoc, &start_date, &end)?;
                }
                DayAction::Check { scope } => {
                    if scope != "all" {
                        return Err(anyhow!("日线检查仅支持 all 参数"));
                    }
                    println!("检查全部日线数据");
                    crate::day::check_all(&vipdoc)?;
                }
            },
            Command::Tick { action } => match action {
                TickAction::Create {
                    start_date,
                    end_date,
                } => {
                    let end = end_date.unwrap_or_else(|| start_date.clone());
                    println!("转档分笔数据: {} - {}", start_date, end);
                    crate::tick::create_tick(&vipdoc, &start_date, &end)?;
                }
                TickAction::Del {
                    start_date,
                    end_date,
                } => {
                    let end = end_date.unwrap_or_else(|| start_date.clone());
                    println!("删除分笔数据: {} - {}", start_date, end);
                    crate::tick::del_tick(&vipdoc, &start_date, &end)?;
                }
                TickAction::Check { scope } => {
                    if scope != "all" {
                        return Err(anyhow!("分笔检查仅支持 all 参数"));
                    }
                    println!("检查全部分笔数据");
                    crate::tick::check_all(&vipdoc)?;
                }
            },
            Command::Min { action } => match action {
                MinAction::Create {
                    start_or_all,
                    end_date,
                } => {
                    if start_or_all.as_deref() == Some("all") {
                        println!("全部分笔数据转分钟数据");
                        crate::min::create_min_all(&vipdoc)?;
                    } else {
                        let start = start_or_all
                            .ok_or_else(|| anyhow!("请指定开始日期或 all"))?;
                        let end = end_date.unwrap_or_else(|| start.clone());
                        println!("指定日期分笔数据转分钟: {} - {}", start, end);
                        crate::min::create_min(&vipdoc, &start, &end)?;
                    }
                }
                MinAction::Del {
                    start_date,
                    end_date,
                } => {
                    let end = end_date.unwrap_or_else(|| start_date.clone());
                    println!("删除分钟数据: {} - {}", start_date, end);
                    crate::min::del_min(&vipdoc, &start_date, &end)?;
                }
            },
        }

        Ok(())
    }
}
