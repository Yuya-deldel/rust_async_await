mod async_mod;

fn main() {
    println!("Hello, world!");
    
    let id = async_mod::send_add(1, 2).unwrap();
    println!("id: {}", id);

    std::thread::sleep(std::time::Duration::from_secs(4));
    println!("main sleep done");

    let result = async_mod::get_add(id).unwrap();
    println!("result: {}", result);
}

// test
// 依存性注入: trait を実装した構造体を関数に渡し、関数からは trait の method のみを呼び出す
pub trait AsyncProcess<X, Y, Z> {
    fn send_add(&self, input: X) -> Result<Y, String>;
    fn get_add(&self, key: Y) -> Result<Z, String>;
}

fn do_something<T>(async_handle: T, input: i32) -> Result<i32, String>
    where T: AsyncProcess<i32, String, i32> 
{
    let key = async_handle.send_add(input)?;
    println!("something is happening");
    let result = async_handle.get_add(key)?;
    
    if result > 10 {
        return Err("result is too big".to_string());
    } else if result == 8 {
        return Ok(result * 2)
    }
    Ok(result * 3)
}

#[cfg(test)]
mod get_team_process_tests {
    use super::*;
    use mockall::predicate::*;
    use mockall::mock;

    mock!{
        DatabaseHandler {}
        impl AsyncProcess<i32, String, i32> for DatabaseHandler {
            fn send_add(&self, input: i32) -> Result<String, String>;
            fn get_add(&self, key: String) -> Result<i32, String>;
        }
    }

    #[test]
    fn do_something_fail() {
        let mut handle = MockDatabaseHandler::new();
        handle.expect_spawn().with(eq(4)).returning(|_| {
            Ok("test_key".to_string())
        });
        handle.expect_get_result().with(eq("test_key".to_string())).returning(|_| {
            Ok(11)
        });
        let outcome = do_something(handle, 4);
        assert_eq!(outcome, Err("result is too big".to_string()));
    }
}