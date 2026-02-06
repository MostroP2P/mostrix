use std::fmt::{self, Display};
use std::str::FromStr;

use ratatui::text::Line;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserTab {
    Orders,
    MyTrades,
    Messages,
    Settings,
    CreateNewOrder,
    Exit,
}

impl Display for UserTab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                UserTab::Orders => "Orders",
                UserTab::MyTrades => "My Trades",
                UserTab::Messages => "Messages",
                UserTab::Settings => "Settings",
                UserTab::CreateNewOrder => "Create New Order",
                UserTab::Exit => "Exit",
            }
        )
    }
}

impl UserTab {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => UserTab::Orders,
            1 => UserTab::MyTrades,
            2 => UserTab::Messages,
            3 => UserTab::CreateNewOrder,
            4 => UserTab::Settings,
            5 => UserTab::Exit,
            _ => panic!("Invalid user tab index: {}", index),
        }
    }

    pub fn as_index(self) -> usize {
        match self {
            UserTab::Orders => 0,
            UserTab::MyTrades => 1,
            UserTab::Messages => 2,
            UserTab::CreateNewOrder => 3,
            UserTab::Settings => 4,
            UserTab::Exit => 5,
        }
    }

    pub fn count() -> usize {
        6
    }

    pub fn first() -> Self {
        UserTab::Orders
    }

    pub fn last() -> Self {
        UserTab::Exit
    }

    pub fn prev(self) -> Self {
        match self {
            UserTab::Orders => UserTab::Orders,
            UserTab::MyTrades => UserTab::Orders,
            UserTab::Messages => UserTab::MyTrades,
            UserTab::CreateNewOrder => UserTab::Messages,
            UserTab::Settings => UserTab::CreateNewOrder,
            UserTab::Exit => UserTab::Settings,
        }
    }

    pub fn next(self) -> Self {
        match self {
            UserTab::Orders => UserTab::MyTrades,
            UserTab::MyTrades => UserTab::Messages,
            UserTab::Messages => UserTab::CreateNewOrder,
            UserTab::CreateNewOrder => UserTab::Settings,
            UserTab::Settings => UserTab::Exit,
            UserTab::Exit => UserTab::Exit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminTab {
    DisputesPending,
    DisputesInProgress,
    Settings,
    Exit,
}

impl Display for AdminTab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                AdminTab::DisputesPending => "Disputes Pending",
                AdminTab::DisputesInProgress => "Disputes Management",
                AdminTab::Settings => "Settings",
                AdminTab::Exit => "Exit",
            }
        )
    }
}

impl AdminTab {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => AdminTab::DisputesPending,
            1 => AdminTab::DisputesInProgress,
            2 => AdminTab::Settings,
            3 => AdminTab::Exit,
            _ => panic!("Invalid admin tab index: {}", index),
        }
    }

    pub fn as_index(self) -> usize {
        match self {
            AdminTab::DisputesPending => 0,
            AdminTab::DisputesInProgress => 1,
            AdminTab::Settings => 2,
            AdminTab::Exit => 3,
        }
    }

    pub fn count() -> usize {
        4
    }

    pub fn first() -> Self {
        AdminTab::DisputesPending
    }

    pub fn last() -> Self {
        AdminTab::Exit
    }

    pub fn prev(self) -> Self {
        match self {
            AdminTab::DisputesPending => AdminTab::DisputesPending,
            AdminTab::DisputesInProgress => AdminTab::DisputesPending,
            AdminTab::Settings => AdminTab::DisputesInProgress,
            AdminTab::Exit => AdminTab::Settings,
        }
    }

    pub fn next(self) -> Self {
        match self {
            AdminTab::DisputesPending => AdminTab::DisputesInProgress,
            AdminTab::DisputesInProgress => AdminTab::Settings,
            AdminTab::Settings => AdminTab::Exit,
            AdminTab::Exit => AdminTab::Exit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserRole {
    User,
    Admin,
}

impl Display for UserRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                UserRole::User => "user",
                UserRole::Admin => "admin",
            }
        )
    }
}

impl FromStr for UserRole {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "user" => Ok(UserRole::User),
            "admin" => Ok(UserRole::Admin),
            _ => Err(anyhow::anyhow!("Invalid user role: {s}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    User(UserTab),
    Admin(AdminTab),
}

impl Display for Tab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tab::User(tab) => write!(f, "{}", tab),
            Tab::Admin(tab) => write!(f, "{}", tab),
        }
    }
}

impl Tab {
    pub fn from_index(index: usize, role: UserRole) -> Self {
        match role {
            UserRole::User => Tab::User(UserTab::from_index(index)),
            UserRole::Admin => Tab::Admin(AdminTab::from_index(index)),
        }
    }

    pub fn as_index(self) -> usize {
        match self {
            Tab::User(tab) => tab.as_index(),
            Tab::Admin(tab) => tab.as_index(),
        }
    }

    pub fn to_line<'a>(self) -> Line<'a> {
        Line::from(self.to_string())
    }

    pub fn count(role: UserRole) -> usize {
        match role {
            UserRole::User => UserTab::count(),
            UserRole::Admin => AdminTab::count(),
        }
    }

    pub fn first(role: UserRole) -> Self {
        match role {
            UserRole::User => Tab::User(UserTab::first()),
            UserRole::Admin => Tab::Admin(AdminTab::first()),
        }
    }

    pub fn last(role: UserRole) -> Self {
        match role {
            UserRole::User => Tab::User(UserTab::last()),
            UserRole::Admin => Tab::Admin(AdminTab::last()),
        }
    }

    pub fn prev(self, role: UserRole) -> Self {
        match (self, role) {
            (Tab::User(tab), UserRole::User) => Tab::User(tab.prev()),
            (Tab::Admin(tab), UserRole::Admin) => Tab::Admin(tab.prev()),
            _ => self, // Invalid combination, return self
        }
    }

    pub fn next(self, role: UserRole) -> Self {
        match (self, role) {
            (Tab::User(tab), UserRole::User) => Tab::User(tab.next()),
            (Tab::Admin(tab), UserRole::Admin) => Tab::Admin(tab.next()),
            _ => self, // Invalid combination, return self
        }
    }

    pub fn get_titles(role: UserRole) -> Vec<String> {
        match role {
            UserRole::User => (0..UserTab::count())
                .map(|i| UserTab::from_index(i).to_string())
                .collect(),
            UserRole::Admin => (0..AdminTab::count())
                .map(|i| AdminTab::from_index(i).to_string())
                .collect(),
        }
    }
}
